//! An agent that runs inside the test process: it writes and reads the same
//! JSON-RPC frames a real ACP agent does, over a pair of pipes instead of a
//! child process. That makes the harness testable without installing anything,
//! without a network, and without the process-tree and console-window machinery
//! `spawn` needs.
//!
//! It can answer as a v2 agent or a v1 one, so both sides of the negotiation
//! are exercised by the same peer.

use super::*;

/// The agent's end of the two pipes.
struct Peer {
    /// Writes to Vivianne.
    out: tokio::io::DuplexStream,
    /// Reads what Vivianne writes.
    inp: tokio::io::BufReader<tokio::io::DuplexStream>,
}

/// What a v2 agent that does everything Vivianne asks for advertises: sessions,
/// images, and both MCP transports.
fn v2_caps() -> Value {
    json!({
        "session": {
            "prompt": { "image": {}, "audio": {}, "embeddedContext": {} },
            "mcp": { "stdio": {}, "http": {} },
        },
    })
}

impl Peer {
    /// The next message Vivianne sends. Panics rather than hang the suite.
    async fn recv(&mut self) -> Value {
        let mut line = String::new();
        let n = tokio::time::timeout(Duration::from_secs(10), self.inp.read_line(&mut line))
            .await
            .expect("Vivianne didn't send anything")
            .expect("reading Vivianne's side failed");
        assert!(n > 0, "Vivianne closed the connection");
        serde_json::from_str(&line).expect("Vivianne sent something that isn't JSON")
    }

    /// The next frame if Vivianne sends one within `ms`; `None` when it stays quiet.
    async fn next_within(&mut self, ms: u64) -> Option<Value> {
        let mut line = String::new();
        match tokio::time::timeout(Duration::from_millis(ms), self.inp.read_line(&mut line)).await {
            Ok(Ok(n)) if n > 0 => Some(serde_json::from_str(&line).expect("Vivianne sent something that isn't JSON")),
            _ => None,
        }
    }

    async fn send(&mut self, msg: Value) {
        let mut line = serde_json::to_string(&msg).expect("a frame that serialises");
        line.push('\n');
        self.out.write_all(line.as_bytes()).await.expect("writing to Vivianne failed");
        self.out.flush().await.expect("flushing to Vivianne failed");
    }

    async fn answer(&mut self, id: &Value, result: Value) {
        self.send(json!({ "jsonrpc": "2.0", "id": id, "result": result })).await;
    }

    async fn refuse(&mut self, id: &Value, code: i64, message: &str, data: Value) {
        self.send(json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message, "data": data } })).await;
    }

    /// Answer `initialize` as an agent of `version`, in that version's own
    /// shape, and return what Vivianne asked with.
    async fn hello_version(&mut self, version: u64, capabilities: Value, auth: Value) -> Value {
        let msg = self.recv().await;
        assert_eq!(msg["method"], "initialize", "expected initialize, got {msg}");
        let answer = if version >= 2 {
            json!({ "protocolVersion": version, "capabilities": capabilities, "info": { "name": "mock", "version": "0.0.0" }, "authMethods": auth })
        } else {
            json!({ "protocolVersion": version, "agentCapabilities": capabilities, "authMethods": auth })
        };
        self.answer(&msg["id"], answer).await;
        msg
    }

    /// Answer `initialize` as a v2 agent with the usual capabilities.
    async fn hello(&mut self) -> Value {
        self.hello_version(2, v2_caps(), json!([])).await
    }

    /// Answer `initialize` as a v1 agent.
    async fn hello_v1(&mut self, capabilities: Value) -> Value {
        self.hello_version(1, capabilities, json!([])).await
    }

    /// Refuse `initialize` with a JSON-RPC error.
    async fn refuse_hello(&mut self, code: i64, message: &str, data: Value) {
        let msg = self.recv().await;
        assert_eq!(msg["method"], "initialize", "expected initialize, got {msg}");
        self.refuse(&msg["id"], code, message, data).await;
    }

    /// Answer `session/new` and return what Vivianne asked with.
    async fn open(&mut self, session: &str) -> Value {
        let msg = self.recv().await;
        assert_eq!(msg["method"], "session/new", "expected session/new, got {msg}");
        self.answer(&msg["id"], json!({ "sessionId": session })).await;
        msg
    }

    /// Answer a `session/resume` or `session/load` and return what Vivianne
    /// asked with. The reply body is empty: the harness keeps the id it sent.
    async fn reopen(&mut self, method: &str) -> Value {
        let msg = self.recv().await;
        assert_eq!(msg["method"], method, "expected {method}, got {msg}");
        self.answer(&msg["id"], json!({})).await;
        msg
    }

    /// The turn's `session/prompt`, left unanswered so the test can stream
    /// updates first and end the turn when it likes.
    async fn prompt(&mut self) -> Value {
        let msg = self.recv().await;
        assert_eq!(msg["method"], "session/prompt", "expected session/prompt, got {msg}");
        msg
    }

    async fn update(&mut self, session: &str, update: Value) {
        self.send(json!({ "jsonrpc": "2.0", "method": "session/update", "params": { "sessionId": session, "update": update } })).await;
    }

    /// Send a request of the agent's own and leave it unanswered, so the test
    /// can send more before the user has decided.
    async fn send_request(&mut self, id: u64, method: &str, params: Value) {
        self.send(json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params })).await;
    }

    /// Wait for Vivianne's answer to request `id`. The other frames it wrote
    /// meanwhile (a `session/cancel`, say) come back with it.
    async fn await_response(&mut self, id: u64) -> (Value, Vec<Value>) {
        let mut others = Vec::new();
        loop {
            let msg = self.recv().await;
            if msg.get("method").is_none() && msg["id"] == json!(id) {
                return (msg.get("result").cloned().unwrap_or(Value::Null), others);
            }
            others.push(msg);
        }
    }

    async fn call(&mut self, id: u64, method: &str, params: Value) -> (Value, Vec<Value>) {
        self.send_request(id, method, params).await;
        self.await_response(id).await
    }

    /// Ask for permission and return the outcome Vivianne replies with.
    async fn ask(&mut self, session: &str, tool_call: Value, options: Value) -> Value {
        let id = json!(99);
        self.send(json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "session/request_permission",
            "params": { "sessionId": session, "toolCall": tool_call, "options": options },
        }))
        .await;
        loop {
            let msg = self.recv().await;
            if msg.get("method").is_none() && msg["id"] == id {
                return msg.get("result").cloned().unwrap_or(Value::Null);
            }
        }
    }
}

/// A connected harness whose agent is this test process.
fn peer(name: &str) -> (Arc<Conn>, Peer) {
    let (client_out, agent_in) = tokio::io::duplex(64 * 1024);
    let (agent_out, client_in) = tokio::io::duplex(64 * 1024);
    let conn = wire(
        Box::new(client_out),
        Box::new(BufReader::new(client_in)),
        None,
        // No process, so nothing to kill: a no-op drop.
        crate::agent::runtime::streaming::TreeKillOnDrop { pid: None },
        name.to_string(),
    );
    (conn, Peer { out: agent_out, inp: BufReader::new(agent_in) })
}

/// The harness entry these tests run as. `spawn` is never reached: the
/// connection comes from `peer`.
fn agent(id: &str) -> Harness {
    Harness {
        id: id.into(),
        name: "Mock agent".into(),
        description: String::new(),
        icon: None,
        available: true,
        ready: true,
        cmd: "mock".into(),
        args: Vec::new(),
        env: HashMap::new(),
        package: None,
        extra: Vec::new(),
        managed: true,
    }
}

/// A fresh thread id: the `delegate-` prefix keeps the session out of
/// `~/.viv/acp-sessions.json`.
fn thread() -> String {
    format!("{DELEGATE}{}", uuid::Uuid::new_v4())
}

/// Start one turn over `conn`: its task and the events it emits. `abort` is
/// the turn's Stop flag.
async fn start_turn(
    h: &Harness,
    conn: &Arc<Conn>,
    session: &str,
    thread: &str,
    text: &str,
    bypass: bool,
    plan: bool,
    abort: Arc<AtomicBool>,
) -> (tokio::task::JoinHandle<Vec<AgentMessage>>, crate::agent::runtime::events::EventReceiver) {
    let state = crate::agent::runtime::state::AgentState::new();
    state.set_plan_mode(plan).await;
    let (tx, rx) = crate::agent::runtime::events::event_bus();
    let user = AgentMessage::User { content: vec![MessageContent::Text { text: text.into() }], timestamp: 0 };
    let (h, conn, session, thread) = (h.clone(), conn.clone(), session.to_string(), thread.to_string());
    let task = tokio::spawn(async move {
        // Nothing was said before this turn, so the agent gets no digest.
        turn_on(&h, &conn, &session, &thread, &state, &tx, &user, &abort, bypass, Vec::new(), vec![user.clone()]).await
    });
    (task, rx)
}

/// Run one turn over `conn` and collect its events, answering any permission
/// prompt the way `approve` says. Returns the transcript and the events.
async fn turn(h: &Harness, conn: &Arc<Conn>, session: &str, thread: &str, text: &str, bypass: bool, approve: bool) -> (Vec<AgentMessage>, Vec<AgentEvent>) {
    turn_in(h, conn, session, thread, text, bypass, false, approve).await
}

/// `turn`, in plan mode or not.
#[allow(clippy::too_many_arguments)]
async fn turn_in(h: &Harness, conn: &Arc<Conn>, session: &str, thread: &str, text: &str, bypass: bool, plan: bool, approve: bool) -> (Vec<AgentMessage>, Vec<AgentEvent>) {
    let (task, mut rx) = start_turn(h, conn, session, thread, text, bypass, plan, Arc::new(AtomicBool::new(false))).await;
    let mut events = Vec::new();
    while let Some(e) = rx.rx.recv().await {
        if let AgentEvent::PermissionAsked { request_id, .. } = &e {
            crate::agent::tools::exec::respond_to_permission(request_id, approve);
        }
        let end = matches!(e, AgentEvent::AgentEnd { .. });
        events.push(e);
        if end {
            break;
        }
    }
    (task.await.expect("the turn panicked"), events)
}

/// The answer to a v2 `session/prompt`: the id of the message it inserted, and
/// nothing about how the work it started is going.
async fn accepted(peer: &mut Peer, prompt_id: &Value) {
    peer.answer(prompt_id, json!({ "messageId": "m1" })).await;
}

/// A v2 session's `state_update`: what it is doing, and why it stopped.
async fn work_state(peer: &mut Peer, session: &str, state: &str, reason: Option<&str>) {
    let mut update = json!({ "sessionUpdate": "state_update", "state": state });
    if let Some(r) = reason {
        update["stopReason"] = json!(r);
    }
    peer.update(session, update).await;
}

/// End a v2 turn the way an agent does: accept the prompt, then say the session
/// is idle. `reason` is the idle's `stopReason`; `None` leaves it out, which is
/// a turn that finished without saying why.
async fn finish(peer: &mut Peer, session: &str, prompt_id: &Value, reason: Option<&str>) {
    accepted(peer, prompt_id).await;
    work_state(peer, session, "idle", reason).await;
}

fn asked(events: &[AgentEvent]) -> Vec<&AgentEvent> {
    events.iter().filter(|e| matches!(e, AgentEvent::PermissionAsked { .. })).collect()
}

/// Everything the assistant said in `out`, joined.
fn said(out: &[AgentMessage]) -> String {
    out.iter()
        .filter_map(|m| match m {
            AgentMessage::Assistant { content, .. } => Some(
                content
                    .iter()
                    .filter_map(|c| match c {
                        MessageContent::Text { text } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<String>(),
            ),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn initialize_negotiates_and_opens_a_session() {
    let (conn, mut peer) = peer("Mock agent");
    let h = agent("mock");
    let thread = thread();
    let script = tokio::spawn(async move {
        let hello = peer.hello().await;
        assert_eq!(hello["params"]["protocolVersion"], 2, "the newest version Vivianne speaks");
        // Forms are the one thing Vivianne answers for the agent; its own file
        // and shell work needs nothing from the client.
        assert_eq!(hello["params"]["capabilities"], json!({ "elicitation": { "form": {} } }));
        assert!(hello["params"].get("clientCapabilities").is_none(), "that is v1's shape: {hello}");
        assert!(hello["params"].get("clientInfo").is_none(), "that is v1's shape: {hello}");
        assert_eq!(hello["params"]["info"]["name"], "vivianne");
        assert!(hello["params"]["info"]["version"].is_string());
        peer.open("s1").await
    });
    let caps = handshake(&conn, &h).await.expect("the handshake");
    assert_eq!(caps["protocolVersion"], PROTOCOL_VERSION);
    assert_eq!(open_session(&conn, &h, &thread, "/tmp").await.expect("a session"), "s1");
    let opened = script.await.expect("the mock agent");
    assert_eq!(opened["params"]["cwd"], "/tmp");
    // The connection keeps it: a second turn doesn't open another.
    assert_eq!(open_session(&conn, &h, &thread, "/tmp").await.expect("the same session"), "s1");
}

#[tokio::test]
async fn a_v1_agent_is_still_spoken_to_and_its_capabilities_read_from_v1s_fields() {
    let (conn, mut peer) = peer("Mock agent");
    let h = agent("mock");
    let thread = thread();
    let script = tokio::spawn(async move {
        let hello = peer
            .hello_version(
                1,
                json!({ "loadSession": true, "promptCapabilities": { "image": true }, "mcpCapabilities": { "http": true } }),
                json!([]),
            )
            .await;
        // The version can't be known before it answers, so the first ask is
        // always v2's shape; a v1 agent answers with the version it has.
        assert_eq!(hello["params"]["protocolVersion"], 2);
        peer.open("s1").await
    });
    handshake(&conn, &h).await.expect("the handshake");
    assert_eq!(conn.version(), 1);
    assert!(!conn.speaks_v2());
    assert!(conn.takes_images(), "v1's promptCapabilities.image is a boolean");
    assert!(conn.takes_stdio_mcp(), "v1 makes stdio the baseline every agent supports");
    assert!(conn.takes_http_mcp());
    assert_eq!(open_session(&conn, &h, &thread, "/tmp").await.expect("a session"), "s1");
    script.await.expect("the mock agent");
}

#[tokio::test]
async fn an_agent_that_only_speaks_a_newer_version_names_it() {
    let (conn, mut peer) = peer("Mock agent");
    let h = agent("mock");
    let script = tokio::spawn(async move {
        peer.refuse_hello(UNSUPPORTED_VERSION, "unsupported protocol version", json!({ "requested": 2, "supported": [3, 4] })).await;
    });
    let e = handshake(&conn, &h).await.expect_err("no agreement");
    assert!(e.contains("only speaks version 3, 4"), "{e}");
    script.await.expect("the mock agent");
}

#[tokio::test]
async fn an_answer_with_a_version_vivianne_does_not_speak_is_refused() {
    let (conn, mut peer) = peer("Mock agent");
    let h = agent("mock");
    let script = tokio::spawn(async move {
        peer.hello_version(3, json!({}), json!([])).await;
    });
    let e = handshake(&conn, &h).await.expect_err("no agreement");
    assert!(e.contains("version 3") && e.contains("1 to 2"), "{e}");
    assert_eq!(conn.version(), 0, "an unusable version is not kept");
    script.await.expect("the mock agent");
}

#[tokio::test]
async fn a_v2_agent_reopens_the_saved_session_without_replaying_it() {
    let (conn, mut peer) = peer("Mock agent");
    let h = agent("mock");
    let script = tokio::spawn(async move {
        peer.hello().await;
        peer.reopen("session/resume").await
    });
    handshake(&conn, &h).await.expect("the handshake");
    let id = open_with(&conn, &h, Some(("s1".into(), 7)), "/tmp").await.expect("the session");
    let asked = script.await.expect("the mock agent");
    assert_eq!(id, "s1");
    assert_eq!(asked["params"]["sessionId"], "s1");
    assert_eq!(asked["params"]["cwd"], "/tmp");
    assert!(asked["params"].get("replayFrom").is_none(), "Vivianne doesn't re-render history, and the turn's digest covers the gap: {asked}");
    assert_eq!(conn.seen.load(Ordering::SeqCst), 7, "it keeps the place in the thread");
}

#[tokio::test]
async fn a_v1_agent_that_can_load_reopens_with_session_load() {
    let (conn, mut peer) = peer("Mock agent");
    let h = agent("mock");
    let script = tokio::spawn(async move {
        peer.hello_v1(json!({ "loadSession": true })).await;
        peer.reopen("session/load").await
    });
    handshake(&conn, &h).await.expect("the handshake");
    assert_eq!(open_with(&conn, &h, Some(("s1".into(), 4)), "/tmp").await.expect("the session"), "s1");
    assert_eq!(conn.seen.load(Ordering::SeqCst), 4);
    script.await.expect("the mock agent");
}

#[tokio::test]
async fn a_v1_agent_that_can_resume_reopens_with_session_resume() {
    let (conn, mut peer) = peer("Mock agent");
    let h = agent("mock");
    let script = tokio::spawn(async move {
        peer.hello_v1(json!({ "sessionCapabilities": { "resume": {} } })).await;
        peer.reopen("session/resume").await
    });
    handshake(&conn, &h).await.expect("the handshake");
    assert_eq!(open_with(&conn, &h, Some(("s1".into(), 4)), "/tmp").await.expect("the session"), "s1");
    assert_eq!(conn.seen.load(Ordering::SeqCst), 4);
    script.await.expect("the mock agent");
}

#[tokio::test]
async fn a_v1_agent_that_cannot_reopen_starts_a_new_session() {
    let (conn, mut peer) = peer("Mock agent");
    let h = agent("mock");
    let script = tokio::spawn(async move {
        peer.hello_v1(json!({})).await;
        peer.open("s2").await
    });
    handshake(&conn, &h).await.expect("the handshake");
    assert_eq!(open_with(&conn, &h, Some(("s1".into(), 7)), "/tmp").await.expect("a session"), "s2");
    assert_eq!(conn.seen.load(Ordering::SeqCst), 0, "a new session has seen nothing");
    script.await.expect("the mock agent");
}

#[tokio::test]
async fn a_v2_agent_that_does_not_advertise_sessions_gets_a_new_one() {
    let (conn, mut peer) = peer("Mock agent");
    let h = agent("mock");
    let script = tokio::spawn(async move {
        peer.hello_version(2, json!({}), json!([])).await;
        peer.open("s2").await
    });
    handshake(&conn, &h).await.expect("the handshake");
    assert_eq!(open_with(&conn, &h, Some(("s1".into(), 7)), "/tmp").await.expect("a session"), "s2");
    script.await.expect("the mock agent");
}

#[tokio::test]
async fn a_resume_the_agent_no_longer_has_starts_a_new_session() {
    let (conn, mut peer) = peer("Mock agent");
    let h = agent("mock");
    let script = tokio::spawn(async move {
        peer.hello().await;
        let msg = peer.recv().await;
        assert_eq!(msg["method"], "session/resume");
        peer.refuse(&msg["id"], -32002, "session not found", Value::Null).await;
        peer.open("s2").await
    });
    handshake(&conn, &h).await.expect("the handshake");
    assert_eq!(open_with(&conn, &h, Some(("s1".into(), 7)), "/tmp").await.expect("a session"), "s2");
    assert_eq!(conn.seen.load(Ordering::SeqCst), 0);
    script.await.expect("the mock agent");
}

#[tokio::test]
async fn a_v2_agent_that_wants_a_sign_in_gets_one_then_the_setup_is_retried() {
    let (conn, mut peer) = peer("Mock agent");
    let h = agent("mock");
    let thread = thread();
    let script = tokio::spawn(async move {
        peer.hello_version(2, v2_caps(), json!([{ "methodId": "oauth", "name": "Sign in with Claude", "type": "agent" }])).await;
        let first = peer.recv().await;
        assert_eq!(first["method"], "session/new");
        peer.refuse(&first["id"], AUTH_REQUIRED, "authentication required", Value::Null).await;
        let login = peer.recv().await;
        assert_eq!(login["method"], "auth/login", "v2 signs in with auth/login: {login}");
        assert_eq!(login["params"]["methodId"], "oauth");
        peer.answer(&login["id"], json!({})).await;
        peer.open("s2").await
    });
    handshake(&conn, &h).await.expect("the handshake");
    assert_eq!(open_session(&conn, &h, &thread, "/tmp").await.expect("a session"), "s2");
    script.await.expect("the mock agent");
}

#[tokio::test]
async fn a_v1_agent_is_signed_in_with_authenticate() {
    let (conn, mut peer) = peer("Mock agent");
    let h = agent("mock");
    let thread = thread();
    let script = tokio::spawn(async move {
        peer.hello_version(1, json!({}), json!([{ "id": "env", "name": "Use ANTHROPIC_API_KEY" }])).await;
        let first = peer.recv().await;
        assert_eq!(first["method"], "session/new");
        peer.refuse(&first["id"], AUTH_REQUIRED, "authentication required", Value::Null).await;
        let login = peer.recv().await;
        assert_eq!(login["method"], "authenticate", "v1 signs in with authenticate: {login}");
        assert_eq!(login["params"]["methodId"], "env");
        peer.answer(&login["id"], json!({})).await;
        peer.open("s2").await
    });
    handshake(&conn, &h).await.expect("the handshake");
    assert_eq!(open_session(&conn, &h, &thread, "/tmp").await.expect("a session"), "s2");
    script.await.expect("the mock agent");
}

#[tokio::test]
async fn an_agent_that_offers_only_a_terminal_sign_in_says_what_to_do() {
    let (conn, mut peer) = peer("Mock agent");
    let h = agent("mock");
    let thread = thread();
    let script = tokio::spawn(async move {
        peer.hello_version(2, v2_caps(), json!([{ "methodId": "browser", "name": "Open a browser", "type": "terminal" }])).await;
        let first = peer.recv().await;
        assert_eq!(first["method"], "session/new");
        peer.refuse(&first["id"], AUTH_REQUIRED, "authentication required", Value::Null).await;
        // A terminal sign-in is the user's to run, not Vivianne's.
        peer.next_within(300).await
    });
    handshake(&conn, &h).await.expect("the handshake");
    let e = open_session(&conn, &h, &thread, "/tmp").await.expect_err("no session");
    assert!(e.contains("terminal sign-in has to be done by hand"), "{e}");
    assert!(script.await.expect("the mock agent").is_none(), "it didn't try anything else");
}

#[tokio::test]
async fn a_reply_streams_into_the_transcript() {
    let (conn, mut peer) = peer("Mock agent");
    let h = agent("mock");
    let thread = thread();
    let script = tokio::spawn(async move {
        // These turns speak v1's update shapes; the v2 ones arrive with Phase 5.
        peer.hello_v1(json!({})).await;
        peer.open("s1").await;
        let p = peer.prompt().await;
        peer.update("s1", json!({ "sessionUpdate": "agent_message_chunk", "content": { "type": "text", "text": "Hel" } })).await;
        peer.update("s1", json!({ "sessionUpdate": "agent_message_chunk", "content": { "type": "text", "text": "lo" } })).await;
        peer.answer(&p["id"], json!({ "stopReason": "end_turn" })).await;
    });
    handshake(&conn, &h).await.expect("the handshake");
    open_session(&conn, &h, &thread, "/tmp").await.expect("a session");
    let (out, events) = turn(&h, &conn, "s1", &thread, "hi", false, true).await;
    script.await.expect("the mock agent");

    assert_eq!(out.len(), 2, "the user's message and one reply: {out:?}");
    match &out[1] {
        AgentMessage::Assistant { content, stop_reason, error_message, .. } => {
            assert_eq!(stop_reason, "stop");
            assert!(error_message.is_none());
            assert_eq!(content.len(), 1);
            assert!(matches!(&content[0], MessageContent::Text { text } if text == "Hello"), "{content:?}");
        }
        other => panic!("expected a reply, got {other:?}"),
    }
    let chunks: Vec<&str> = events.iter().filter_map(|e| if let AgentEvent::MessageUpdate { chunk, .. } = e { Some(chunk.as_str()) } else { None }).collect();
    assert_eq!(chunks, vec!["Hel", "lo"]);
    assert!(matches!(events.last(), Some(AgentEvent::AgentEnd { .. })));
}

#[tokio::test]
async fn a_tool_call_becomes_a_tool_row() {
    let (conn, mut peer) = peer("Mock agent");
    let h = agent("mock");
    let thread = thread();
    let script = tokio::spawn(async move {
        // These turns speak v1's update shapes; the v2 ones arrive with Phase 5.
        peer.hello_v1(json!({})).await;
        peer.open("s1").await;
        let p = peer.prompt().await;
        peer.update("s1", json!({ "sessionUpdate": "tool_call", "toolCallId": "t1", "kind": "execute", "title": "ls", "status": "pending" })).await;
        peer.update(
            "s1",
            json!({
                "sessionUpdate": "tool_call_update",
                "toolCallId": "t1",
                "status": "completed",
                "content": [{ "type": "content", "content": { "type": "text", "text": "a.rs" } }],
            }),
        )
        .await;
        peer.answer(&p["id"], json!({ "stopReason": "end_turn" })).await;
    });
    handshake(&conn, &h).await.expect("the handshake");
    open_session(&conn, &h, &thread, "/tmp").await.expect("a session");
    let (out, events) = turn(&h, &conn, "s1", &thread, "list it", false, true).await;
    script.await.expect("the mock agent");

    let start = events.iter().find_map(|e| if let AgentEvent::ToolExecutionStart { tool_name, args, .. } = e { Some((tool_name, args)) } else { None });
    let (tool_name, args) = start.expect("the call starting");
    assert_eq!(tool_name, "bash");
    assert_eq!(args["command"], "ls");
    let end = events.iter().find_map(|e| if let AgentEvent::ToolExecutionEnd { tool_name, result, is_error, .. } = e { Some((tool_name, result, is_error)) } else { None });
    let (tool_name, result, is_error) = end.expect("the call ending");
    assert_eq!(tool_name, "bash");
    assert!(!is_error);
    assert!(matches!(&result.content[0], MessageContent::Text { text } if text == "a.rs"), "{result:?}");
    assert!(matches!(out.last(), Some(AgentMessage::ToolResult { tool_name, .. }) if tool_name == "bash"), "{out:?}");
}

#[tokio::test]
async fn a_permission_request_goes_through_the_dialog_and_grants_once() {
    let (conn, mut peer) = peer("Mock agent");
    let h = agent("mock");
    let thread = thread();
    let script = tokio::spawn(async move {
        // These turns speak v1's update shapes; the v2 ones arrive with Phase 5.
        peer.hello_v1(json!({})).await;
        peer.open("s1").await;
        let p = peer.prompt().await;
        let outcome = peer
            .ask(
                "s1",
                json!({ "toolCallId": "t1", "kind": "edit", "title": "Write a.rs", "rawInput": { "path": "a.rs" } }),
                json!([
                    { "optionId": "allow-once", "name": "Allow once", "kind": "allow_once" },
                    { "optionId": "allow-always", "name": "Always allow", "kind": "allow_always" },
                    { "optionId": "reject-once", "name": "Reject", "kind": "reject_once" },
                ]),
            )
            .await;
        peer.answer(&p["id"], json!({ "stopReason": "end_turn" })).await;
        outcome
    });
    handshake(&conn, &h).await.expect("the handshake");
    open_session(&conn, &h, &thread, "/tmp").await.expect("a session");
    let (_out, events) = turn(&h, &conn, "s1", &thread, "write it", false, true).await;
    let outcome = script.await.expect("the mock agent");

    let asks = asked(&events);
    assert_eq!(asks.len(), 1, "one dialog: {events:?}");
    match asks[0] {
        AgentEvent::PermissionAsked { tool_name, reason, .. } => {
            assert_eq!(tool_name, "edit");
            assert_eq!(reason.as_deref(), Some("Mock agent wants to: Write a.rs"));
        }
        _ => unreachable!(),
    }
    // Approval covers this call only: never the standing grant the user didn't give.
    assert_eq!(outcome["outcome"]["outcome"], "selected");
    assert_eq!(outcome["outcome"]["optionId"], "allow-once");
}

#[tokio::test]
async fn refusing_picks_the_reject_option() {
    let (conn, mut peer) = peer("Mock agent");
    let h = agent("mock");
    let thread = thread();
    let script = tokio::spawn(async move {
        // These turns speak v1's update shapes; the v2 ones arrive with Phase 5.
        peer.hello_v1(json!({})).await;
        peer.open("s1").await;
        let p = peer.prompt().await;
        let outcome = peer
            .ask(
                "s1",
                json!({ "toolCallId": "t1", "kind": "edit", "title": "Write a.rs" }),
                json!([
                    { "optionId": "allow-once", "name": "Allow once", "kind": "allow_once" },
                    { "optionId": "reject-once", "name": "Reject", "kind": "reject_once" },
                ]),
            )
            .await;
        peer.answer(&p["id"], json!({ "stopReason": "end_turn" })).await;
        outcome
    });
    handshake(&conn, &h).await.expect("the handshake");
    open_session(&conn, &h, &thread, "/tmp").await.expect("a session");
    let (_out, events) = turn(&h, &conn, "s1", &thread, "write it", false, false).await;
    let outcome = script.await.expect("the mock agent");

    assert_eq!(asked(&events).len(), 1, "one dialog: {events:?}");
    assert_eq!(outcome["outcome"]["outcome"], "selected");
    assert_eq!(outcome["outcome"]["optionId"], "reject-once");
}

#[tokio::test]
async fn an_agent_that_only_offers_a_standing_grant_gets_nothing() {
    let (conn, mut peer) = peer("Mock agent");
    let h = agent("mock");
    let thread = thread();
    let script = tokio::spawn(async move {
        // These turns speak v1's update shapes; the v2 ones arrive with Phase 5.
        peer.hello_v1(json!({})).await;
        peer.open("s1").await;
        let p = peer.prompt().await;
        // Bypass mode approves without a dialog, but "allow_always" is a grant
        // the user never gave, so there is no option to pick.
        let outcome = peer
            .ask("s1", json!({ "toolCallId": "t1", "kind": "execute", "title": "rm -rf build" }), json!([{ "optionId": "allow-always", "name": "Always allow", "kind": "allow_always" }]))
            .await;
        peer.answer(&p["id"], json!({ "stopReason": "end_turn" })).await;
        outcome
    });
    handshake(&conn, &h).await.expect("the handshake");
    open_session(&conn, &h, &thread, "/tmp").await.expect("a session");
    let (_out, events) = turn(&h, &conn, "s1", &thread, "clean up", true, true).await;
    let outcome = script.await.expect("the mock agent");

    assert!(asked(&events).is_empty(), "no dialog in bypass mode: {events:?}");
    assert_eq!(outcome["outcome"]["outcome"], "cancelled");
}

// ── Phase 2: what the agent asks of us ───────────────────────────────────

fn offered() -> Value {
    json!([
        { "optionId": "allow-once", "name": "Allow once", "kind": "allow_once" },
        { "optionId": "reject-once", "name": "Reject", "kind": "reject_once" },
    ])
}

#[test]
fn a_permission_is_allowed_asked_or_refused_by_the_kind_of_call() {
    use Disposition::*;
    // Reading never asks, in any mode.
    for kind in ["read", "search", "think"] {
        for (plan, bypass) in [(false, false), (true, false), (false, true)] {
            assert_eq!(disposition(kind, plan, bypass), Allow, "{kind} plan={plan} bypass={bypass}");
        }
    }
    // Anything that changes something asks, unless bypass allows it or plan mode refuses it.
    for kind in ["edit", "delete", "move", "execute", "other"] {
        assert_eq!(disposition(kind, false, false), Ask, "{kind}");
        assert_eq!(disposition(kind, false, true), Allow, "{kind} in bypass");
        assert_eq!(disposition(kind, true, false), Deny, "{kind} in plan mode");
    }
    // Fetching is research: allowed in plan mode, asked about otherwise.
    assert_eq!(disposition("fetch", true, false), Allow);
    assert_eq!(disposition("fetch", false, false), Ask);
    // Switching the agent's own mode is asked about even where a change is refused.
    assert_eq!(disposition("switch_mode", true, false), Ask);
    assert_eq!(disposition("switch_mode", false, true), Allow);
}

#[test]
fn a_permission_request_reads_the_same_in_either_version() {
    // v1: the tool call at the top.
    let v1 = permission_ask(&json!({ "sessionId": "s", "toolCall": { "toolCallId": "t1", "kind": "edit", "title": "Write a.rs", "rawInput": { "path": "a.rs" } }, "options": offered() }), None);
    assert_eq!((v1.call.id.as_str(), v1.call.name.as_str(), v1.kind.as_deref(), v1.title.as_deref()), ("t1", "edit", Some("edit"), Some("Write a.rs")));
    assert_eq!(v1.call.arguments, json!({ "path": "a.rs" }));
    assert_eq!(v1.options.len(), 2);
    // v2: a tool call under `subject`, with the title on the request.
    let v2 = permission_ask(&json!({ "sessionId": "s", "title": "Edit a.rs", "subject": { "type": "tool_call", "toolCall": { "toolCallId": "t2", "kind": "delete" } }, "options": offered() }), None);
    assert_eq!((v2.call.id.as_str(), v2.kind.as_deref(), v2.title.as_deref()), ("t2", Some("delete"), Some("Edit a.rs")));
    // v2: a command about to run is an `execute` call that shows its command.
    let cmd = permission_ask(&json!({ "sessionId": "s", "title": "Run it", "subject": { "type": "command", "command": "ls -la", "cwd": "/tmp", "toolCallId": "t3" }, "options": offered() }), None);
    assert_eq!((cmd.call.id.as_str(), cmd.call.name.as_str(), cmd.kind.as_deref(), cmd.title.as_deref()), ("t3", "bash", Some("execute"), Some("ls -la")), "a command shows itself, not the request's generic title");
    assert_eq!(cmd.call.arguments, json!({ "command": "ls -la", "cwd": "/tmp" }));
    // An agent that leaves the kind off the request: the one it announced the call with stands in.
    let bare = permission_ask(&json!({ "sessionId": "s", "toolCall": { "toolCallId": "t4", "title": "Write b.rs" }, "options": offered() }), Some("edit".into()));
    assert_eq!((bare.call.name.as_str(), bare.kind.as_deref()), ("edit", Some("edit")));
}

#[test]
fn a_form_of_choices_becomes_questions_and_a_form_with_a_text_box_does_not() {
    let schema = json!({ "type": "object", "properties": {
        "db": { "type": "string", "title": "Database", "oneOf": [{ "const": "pg", "title": "Postgres", "description": "The usual" }, { "const": "sqlite", "title": "SQLite" }] },
        "tls": { "type": "boolean", "title": "Use TLS?" },
        "extras": { "type": "array", "title": "Extras", "items": { "type": "string", "enum": ["cache", "queue"] } },
    } });
    let fields = form_fields("Claude Code agent", "Which setup?", &schema).expect("all choices");
    assert_eq!(fields.len(), 3);
    assert!(fields[0].spec.question.starts_with("Which setup?\n\n"), "the agent's own words lead the first question: {}", fields[0].spec.question);
    assert!(!fields[1].spec.question.contains("Which setup?"));
    assert_eq!(fields[0].spec.header, "Claude Code", "a chip header, kept short");
    let db = fields.iter().find(|f| f.key == "db").unwrap();
    assert_eq!(db.spec.options.iter().map(|o| (o.label.as_str(), o.description.as_str())).collect::<Vec<_>>(), vec![("Postgres", "The usual"), ("SQLite", "")]);
    assert!(!db.spec.multi_select && fields.iter().find(|f| f.key == "extras").unwrap().spec.multi_select);

    let answers: Vec<Vec<String>> = fields
        .iter()
        .map(|f| match f.key.as_str() {
            "db" => vec!["SQLite".to_string()],
            "tls" => vec!["Yes".to_string()],
            _ => vec!["cache".to_string(), "queue".to_string()],
        })
        .collect();
    assert_eq!(form_content(&fields, &answers), Some(json!({ "db": "sqlite", "tls": true, "extras": ["cache", "queue"] })));
    assert_eq!(form_content(&fields, &vec![Vec::new(); 3]), None, "nothing picked");

    let text = json!({ "type": "object", "properties": { "name": { "type": "string", "title": "Name" } } });
    assert!(form_fields("A", "", &text).is_none(), "a free-text field has nowhere to go");
    let number = json!({ "type": "object", "properties": { "n": { "type": "integer" } } });
    assert!(form_fields("A", "", &number).is_none());
    let mixed = json!({ "type": "object", "properties": { "tls": { "type": "boolean" }, "name": { "type": "string" } } });
    assert!(form_fields("A", "", &mixed).is_none(), "one field it can't show is enough to decline the form");
    assert!(form_fields("A", "", &json!({ "type": "object", "properties": {} })).is_none());
}

#[tokio::test]
async fn reading_never_asks_and_plan_mode_refuses_a_change_without_asking() {
    // A read goes through without a dialog.
    let (conn, mut peer) = peer("Mock agent");
    let h = agent("mock");
    let thread = thread();
    let script = tokio::spawn(async move {
        peer.hello_v1(json!({})).await;
        peer.open("s1").await;
        let p = peer.prompt().await;
        let outcome = peer.ask("s1", json!({ "toolCallId": "t1", "kind": "read", "title": "Read a.rs" }), offered()).await;
        peer.answer(&p["id"], json!({ "stopReason": "end_turn" })).await;
        outcome
    });
    handshake(&conn, &h).await.expect("the handshake");
    open_session(&conn, &h, &thread, "/tmp").await.expect("a session");
    let (_out, events) = turn(&h, &conn, "s1", &thread, "look", false, false).await;
    let outcome = script.await.expect("the mock agent");
    assert!(asked(&events).is_empty(), "no dialog for a read: {events:?}");
    assert_eq!(outcome["outcome"]["optionId"], "allow-once");

    // A change in plan mode is refused, also without a dialog.
    let (conn, mut peer) = self::peer("Mock agent");
    let thread = self::thread();
    let script = tokio::spawn(async move {
        peer.hello_v1(json!({})).await;
        peer.open("s1").await;
        let p = peer.prompt().await;
        let outcome = peer.ask("s1", json!({ "toolCallId": "t1", "kind": "edit", "title": "Write a.rs" }), offered()).await;
        peer.answer(&p["id"], json!({ "stopReason": "end_turn" })).await;
        outcome
    });
    handshake(&conn, &h).await.expect("the handshake");
    open_session(&conn, &h, &thread, "/tmp").await.expect("a session");
    let (_out, events) = turn_in(&h, &conn, "s1", &thread, "write", false, true, true).await;
    let outcome = script.await.expect("the mock agent");
    assert!(asked(&events).is_empty(), "plan mode refuses before it asks: {events:?}");
    assert_eq!(outcome["outcome"]["outcome"], "selected");
    assert_eq!(outcome["outcome"]["optionId"], "reject-once");
}

#[tokio::test]
async fn a_v2_permission_request_for_a_command_is_asked_as_a_shell_command() {
    let (conn, mut peer) = peer("Mock agent");
    let h = agent("mock");
    let thread = thread();
    let script = tokio::spawn(async move {
        peer.hello().await;
        peer.open("s1").await;
        let p = peer.prompt().await;
        let (outcome, _) = peer
            .call(99, "session/request_permission", json!({ "sessionId": "s1", "title": "Run it", "subject": { "type": "command", "command": "ls -la", "cwd": "/tmp", "toolCallId": "t1" }, "options": offered() }))
            .await;
        finish(&mut peer, "s1", &p["id"], Some("end_turn")).await;
        outcome
    });
    handshake(&conn, &h).await.expect("the handshake");
    open_session(&conn, &h, &thread, "/tmp").await.expect("a session");
    let (_out, events) = turn(&h, &conn, "s1", &thread, "list", false, true).await;
    let outcome = script.await.expect("the mock agent");
    match asked(&events)[..] {
        [AgentEvent::PermissionAsked { tool_name, args, reason, .. }] => {
            assert_eq!(tool_name, "bash");
            assert_eq!(args["command"], "ls -la");
            assert_eq!(reason.as_deref(), Some("Mock agent wants to: ls -la"));
        }
        _ => panic!("expected one dialog: {events:?}"),
    }
    assert_eq!(outcome["outcome"]["optionId"], "allow-once");
}

#[tokio::test]
async fn the_agents_other_updates_keep_flowing_while_a_permission_dialog_is_open() {
    let (conn, mut peer) = peer("Mock agent");
    let h = agent("mock");
    let thread = thread();
    let script = tokio::spawn(async move {
        peer.hello_v1(json!({})).await;
        peer.open("s1").await;
        let p = peer.prompt().await;
        peer.send_request(99, "session/request_permission", json!({ "sessionId": "s1", "toolCall": { "toolCallId": "t1", "kind": "edit", "title": "Write a.rs" }, "options": offered() })).await;
        peer.update("s1", json!({ "sessionUpdate": "agent_message_chunk", "content": { "type": "text", "text": "meanwhile" } })).await;
        let (outcome, _) = peer.await_response(99).await;
        peer.answer(&p["id"], json!({ "stopReason": "end_turn" })).await;
        outcome
    });
    handshake(&conn, &h).await.expect("the handshake");
    open_session(&conn, &h, &thread, "/tmp").await.expect("a session");
    let (task, mut rx) = start_turn(&h, &conn, "s1", &thread, "write", false, false, Arc::new(AtomicBool::new(false))).await;
    let (mut dialog, mut chunk) = (None, false);
    // Nothing answers the dialog until the update has arrived: a layout that
    // waits on the dialog first never gets there, and this times out.
    tokio::time::timeout(Duration::from_secs(5), async {
        while !(dialog.is_some() && chunk) {
            let e = rx.rx.recv().await.expect("the turn's events");
            match &e {
                AgentEvent::PermissionAsked { request_id, .. } => dialog = Some(request_id.clone()),
                AgentEvent::MessageUpdate { chunk: c, .. } if c == "meanwhile" => chunk = true,
                _ => {}
            }
        }
    })
    .await
    .expect("the update arrived while the dialog was open");
    crate::agent::tools::exec::respond_to_permission(&dialog.unwrap(), true);
    while let Some(e) = rx.rx.recv().await {
        if matches!(e, AgentEvent::AgentEnd { .. }) {
            break;
        }
    }
    task.await.expect("the turn");
    assert_eq!(script.await.expect("the mock agent")["outcome"]["optionId"], "allow-once");
}

#[tokio::test]
async fn stop_withdraws_a_permission_dialog_and_tells_the_agent_it_was_cancelled() {
    let (conn, mut peer) = peer("Mock agent");
    let h = agent("mock");
    let thread = thread();
    let script = tokio::spawn(async move {
        peer.hello_v1(json!({})).await;
        peer.open("s1").await;
        let p = peer.prompt().await;
        let (outcome, mut others) = peer.call(99, "session/request_permission", json!({ "sessionId": "s1", "toolCall": { "toolCallId": "t1", "kind": "edit", "title": "Write a.rs" }, "options": offered() })).await;
        // The cancel and the answer leave Vivianne on two tasks; either may come first.
        while !others.iter().any(|m| m["method"] == "session/cancel") {
            match peer.next_within(2000).await {
                Some(m) => others.push(m),
                None => break,
            }
        }
        peer.answer(&p["id"], json!({ "stopReason": "cancelled" })).await;
        (outcome, others)
    });
    handshake(&conn, &h).await.expect("the handshake");
    open_session(&conn, &h, &thread, "/tmp").await.expect("a session");
    let abort = Arc::new(AtomicBool::new(false));
    let (task, mut rx) = start_turn(&h, &conn, "s1", &thread, "write", false, false, abort.clone()).await;
    let mut events = Vec::new();
    while let Some(e) = rx.rx.recv().await {
        // Stop is pressed with the dialog open and nobody has answered it.
        if matches!(e, AgentEvent::PermissionAsked { .. }) {
            abort.store(true, Ordering::SeqCst);
        }
        let end = matches!(e, AgentEvent::AgentEnd { .. });
        events.push(e);
        if end {
            break;
        }
    }
    task.await.expect("the turn");
    let (outcome, others) = script.await.expect("the mock agent");

    assert_eq!(outcome["outcome"]["outcome"], "cancelled", "the protocol asks a client that cancels to answer open requests this way");
    assert!(others.iter().any(|m| m["method"] == "session/cancel"), "the agent is told to stop: {others:?}");
    assert!(events.iter().any(|e| matches!(e, AgentEvent::PermissionResolved { approved: false, .. })), "the card is withdrawn: {events:?}");
}

#[tokio::test]
async fn a_form_of_choices_is_asked_in_the_question_dialog_and_answered_with_the_picks() {
    let (conn, mut peer) = peer("Mock agent");
    let h = agent("mock");
    let thread = thread();
    let script = tokio::spawn(async move {
        peer.hello().await;
        peer.open("s1").await;
        let p = peer.prompt().await;
        let schema = json!({ "type": "object", "properties": {
            "db": { "type": "string", "title": "Database", "oneOf": [{ "const": "pg", "title": "Postgres" }, { "const": "sqlite", "title": "SQLite" }] },
            "tls": { "type": "boolean", "title": "Use TLS?" },
        } });
        let (answer, _) = peer.call(98, "elicitation/create", json!({ "sessionId": "s1", "mode": "form", "message": "Which setup?", "requestedSchema": schema })).await;
        finish(&mut peer, "s1", &p["id"], Some("end_turn")).await;
        answer
    });
    handshake(&conn, &h).await.expect("the handshake");
    open_session(&conn, &h, &thread, "/tmp").await.expect("a session");
    let (task, mut rx) = start_turn(&h, &conn, "s1", &thread, "set it up", false, false, Arc::new(AtomicBool::new(false))).await;
    let mut asked_questions = None;
    while let Some(e) = rx.rx.recv().await {
        if let AgentEvent::QuestionAsked { request_id, questions, .. } = &e {
            let answers: Vec<Vec<String>> = questions.iter().map(|q| if q.question.contains("Database") { vec!["SQLite".to_string()] } else { vec!["Yes".to_string()] }).collect();
            asked_questions = Some(questions.clone());
            crate::agent::tools::exec::respond_to_question(request_id, answers);
        }
        if matches!(e, AgentEvent::AgentEnd { .. }) {
            break;
        }
    }
    task.await.expect("the turn");
    let answer = script.await.expect("the mock agent");
    let questions = asked_questions.expect("the form was asked as questions");
    assert_eq!(questions.len(), 2);
    assert!(questions.iter().any(|q| q.question.contains("Which setup?")), "the agent's own words are shown");
    assert_eq!(answer, json!({ "action": "accept", "content": { "db": "sqlite", "tls": true } }));
}

#[tokio::test]
async fn a_form_with_a_text_box_is_declined_and_a_stop_cancels_one_that_is_open() {
    // A text field has nowhere to go: declined at once, nothing asked.
    let (conn, mut peer) = peer("Mock agent");
    let h = agent("mock");
    let thread = thread();
    let script = tokio::spawn(async move {
        peer.hello().await;
        peer.open("s1").await;
        let p = peer.prompt().await;
        let (answer, _) = peer
            .call(98, "elicitation/create", json!({ "sessionId": "s1", "mode": "form", "message": "Name?", "requestedSchema": { "type": "object", "properties": { "name": { "type": "string", "title": "Name" } } } }))
            .await;
        finish(&mut peer, "s1", &p["id"], Some("end_turn")).await;
        answer
    });
    handshake(&conn, &h).await.expect("the handshake");
    open_session(&conn, &h, &thread, "/tmp").await.expect("a session");
    let (_out, events) = turn(&h, &conn, "s1", &thread, "ask me", false, true).await;
    assert_eq!(script.await.expect("the mock agent"), json!({ "action": "decline" }));
    assert!(!events.iter().any(|e| matches!(e, AgentEvent::QuestionAsked { .. })), "{events:?}");

    // Stop while the question is open: the form is cancelled and the dialog withdrawn.
    let (conn, mut peer) = self::peer("Mock agent");
    let thread = self::thread();
    let script = tokio::spawn(async move {
        peer.hello().await;
        peer.open("s1").await;
        let p = peer.prompt().await;
        let (answer, _) = peer
            .call(98, "elicitation/create", json!({ "sessionId": "s1", "mode": "form", "message": "Ok?", "requestedSchema": { "type": "object", "properties": { "go": { "type": "boolean", "title": "Go?" } } } }))
            .await;
        finish(&mut peer, "s1", &p["id"], Some("cancelled")).await;
        answer
    });
    handshake(&conn, &h).await.expect("the handshake");
    open_session(&conn, &h, &thread, "/tmp").await.expect("a session");
    let abort = Arc::new(AtomicBool::new(false));
    let (task, mut rx) = start_turn(&h, &conn, "s1", &thread, "ask me", false, false, abort.clone()).await;
    let mut resolved = false;
    while let Some(e) = rx.rx.recv().await {
        if matches!(e, AgentEvent::QuestionAsked { .. }) {
            abort.store(true, Ordering::SeqCst);
        }
        resolved |= matches!(e, AgentEvent::QuestionResolved { .. });
        if matches!(e, AgentEvent::AgentEnd { .. }) {
            break;
        }
    }
    task.await.expect("the turn");
    assert_eq!(script.await.expect("the mock agent"), json!({ "action": "cancel" }));
    assert!(resolved, "the dialog is withdrawn");
}

/// The `stop_reason` the turn's transcript ended on.
fn ended_on(out: &[AgentMessage]) -> Option<&str> {
    out.iter().rev().find_map(|m| match m {
        AgentMessage::Assistant { stop_reason, .. } => Some(stop_reason.as_str()),
        _ => None,
    })
}

/// Drive one turn until Stop is pressed on its first streamed text, then
/// return the transcript. `press` is the test's own Stop flag.
async fn stop_the_turn(h: &Harness, conn: &Arc<Conn>, session: &str, thread: &str) -> Vec<AgentMessage> {
    let abort = Arc::new(AtomicBool::new(false));
    let (task, mut rx) = start_turn(h, conn, session, thread, "go", false, false, abort.clone()).await;
    while let Some(e) = rx.rx.recv().await {
        if matches!(e, AgentEvent::MessageUpdate { .. }) {
            abort.store(true, Ordering::SeqCst);
        }
        if matches!(e, AgentEvent::AgentEnd { .. }) {
            break;
        }
    }
    task.await.expect("the turn")
}

/// The frames Vivianne wrote until both `session/cancel` and `session/close`
/// have arrived, answering the close. Stop writes them on tasks of their own,
/// so either may come first.
async fn until_cancel_and_close(peer: &mut Peer) -> Vec<Value> {
    let mut seen: Vec<Value> = Vec::new();
    while !(seen.iter().any(|m| m["method"] == "session/cancel") && seen.iter().any(|m| m["method"] == "session/close")) {
        seen.push(peer.next_within(3000).await.expect("the cancel and the close"));
    }
    let close = seen.iter().find(|m| m["method"] == "session/close").expect("a close");
    peer.answer(&close["id"], json!({})).await;
    seen
}

#[tokio::test]
async fn stop_closes_a_v2_session_and_does_not_reuse_it() {
    let (conn, mut peer) = peer("Mock agent");
    let h = agent("mock");
    let thread = thread();
    let script = tokio::spawn(async move {
        peer.hello().await;
        peer.open("s1").await;
        let p = peer.prompt().await;
        peer.update("s1", json!({ "sessionUpdate": "agent_message_chunk", "content": { "type": "text", "text": "working" } })).await;
        let seen = until_cancel_and_close(&mut peer).await;
        finish(&mut peer, "s1", &p["id"], Some("cancelled")).await;
        seen
    });
    handshake(&conn, &h).await.expect("the handshake");
    open_session(&conn, &h, &thread, "/tmp").await.expect("a session");
    let out = stop_the_turn(&h, &conn, "s1", &thread).await;
    let seen = script.await.expect("the mock agent");

    let close = seen.iter().find(|m| m["method"] == "session/close").expect("v2 has `session/close` in every agent");
    assert_eq!(close["params"], json!({ "sessionId": "s1" }));
    assert!(conn.session.lock().await.is_none(), "the closed session is not handed to the next turn");
    assert_eq!(ended_on(&out), Some("aborted"), "a stop is not an error: {out:?}");
}

#[tokio::test]
async fn a_v1_agent_without_close_is_only_cancelled_and_keeps_its_session() {
    let (conn, mut peer) = peer("Mock agent");
    let h = agent("mock");
    let thread = thread();
    let script = tokio::spawn(async move {
        peer.hello_v1(json!({})).await;
        peer.open("s1").await;
        let p = peer.prompt().await;
        peer.update("s1", json!({ "sessionUpdate": "agent_message_chunk", "content": { "type": "text", "text": "working" } })).await;
        let cancel = peer.next_within(3000).await.expect("the cancel");
        assert_eq!(cancel["method"], "session/cancel");
        // v1 agents only get a close if they say they have one.
        assert!(peer.next_within(500).await.is_none(), "nothing else is written to a v1 agent without `close`");
        peer.answer(&p["id"], json!({ "stopReason": "cancelled" })).await;
    });
    handshake(&conn, &h).await.expect("the handshake");
    open_session(&conn, &h, &thread, "/tmp").await.expect("a session");
    let out = stop_the_turn(&h, &conn, "s1", &thread).await;
    script.await.expect("the mock agent");

    assert_eq!(conn.session.lock().await.as_deref(), Some("s1"), "a session it can still resume is kept");
    assert_eq!(ended_on(&out), Some("aborted"));
}

#[tokio::test]
async fn a_v1_agent_that_says_it_can_close_is_closed() {
    let (conn, mut peer) = peer("Mock agent");
    let h = agent("mock");
    let thread = thread();
    let script = tokio::spawn(async move {
        peer.hello_v1(json!({ "sessionCapabilities": { "close": {} } })).await;
        peer.open("s1").await;
        let p = peer.prompt().await;
        peer.update("s1", json!({ "sessionUpdate": "agent_message_chunk", "content": { "type": "text", "text": "working" } })).await;
        let seen = until_cancel_and_close(&mut peer).await;
        peer.answer(&p["id"], json!({ "stopReason": "cancelled" })).await;
        seen
    });
    handshake(&conn, &h).await.expect("the handshake");
    open_session(&conn, &h, &thread, "/tmp").await.expect("a session");
    stop_the_turn(&h, &conn, "s1", &thread).await;
    script.await.expect("the mock agent");

    assert!(conn.session.lock().await.is_none());
}

#[tokio::test]
async fn a_turn_that_ignores_stop_is_ended_when_its_time_is_up() {
    let (conn, mut peer) = peer("Mock agent");
    // The real wait is five seconds; a test doesn't have to sit through it.
    conn.stop_kill_after.store(200, Ordering::SeqCst);
    let h = agent("mock");
    let thread = thread();
    let script = tokio::spawn(async move {
        peer.hello().await;
        peer.open("s1").await;
        let _p = peer.prompt().await;
        peer.update("s1", json!({ "sessionUpdate": "agent_message_chunk", "content": { "type": "text", "text": "working" } })).await;
        // It reads the cancel and the close, then goes on working.
        let seen = until_cancel_and_close(&mut peer).await;
        seen
    });
    handshake(&conn, &h).await.expect("the handshake");
    open_session(&conn, &h, &thread, "/tmp").await.expect("a session");
    let started = tokio::time::Instant::now();
    let out = stop_the_turn(&h, &conn, "s1", &thread).await;
    script.await.expect("the mock agent");

    assert!(started.elapsed() < Duration::from_secs(4), "Stop does not wait on an agent that never answers: {:?}", started.elapsed());
    assert_eq!(ended_on(&out), Some("aborted"));
    assert!(conn.session.lock().await.is_none(), "the session died with the turn");
}

#[tokio::test]
async fn a_peer_that_dies_after_stop_is_a_stopped_turn_not_an_error() {
    let (conn, mut peer) = peer("Mock agent");
    let h = agent("mock");
    let thread = thread();
    let script = tokio::spawn(async move {
        peer.hello().await;
        peer.open("s1").await;
        let _p = peer.prompt().await;
        peer.update("s1", json!({ "sessionUpdate": "agent_message_chunk", "content": { "type": "text", "text": "working" } })).await;
        let cancel = peer.next_within(3000).await.expect("the cancel");
        assert_eq!(cancel["method"], "session/cancel");
        // It exits without answering the prompt: the transport fails.
    });
    handshake(&conn, &h).await.expect("the handshake");
    open_session(&conn, &h, &thread, "/tmp").await.expect("a session");
    let out = stop_the_turn(&h, &conn, "s1", &thread).await;
    script.await.expect("the mock agent");

    assert!(!out.iter().any(|m| matches!(m, AgentMessage::Assistant { error_message: Some(_), .. })), "a stop is not a failure: {out:?}");
    assert_eq!(ended_on(&out), Some("aborted"));
}

#[tokio::test]
async fn a_failure_carries_what_the_agent_said_on_stderr() {
    let (conn, mut peer) = peer("Mock agent");
    let h = agent("mock");
    let thread = thread();
    conn.remember_stderr("claude: couldn't reach the model (sk-abcdefghij)");
    let _ = conn.stderr_done.send_replace(true);
    let script = tokio::spawn(async move {
        peer.hello().await;
        peer.open("s1").await;
        let _p = peer.prompt().await;
        // It exits without answering: the turn fails on the transport.
    });
    handshake(&conn, &h).await.expect("the handshake");
    open_session(&conn, &h, &thread, "/tmp").await.expect("a session");
    let (out, _events) = turn(&h, &conn, "s1", &thread, "go", false, true).await;
    script.await.expect("the mock agent");

    let said = out.iter().find_map(|m| match m {
        AgentMessage::Assistant { error_message: Some(e), .. } => Some(e.clone()),
        _ => None,
    });
    let said = said.expect("the failure is on the transcript");
    assert!(said.contains("couldn't reach the model"), "its last words are shown: {said}");
    assert!(said.contains("[redacted]"), "a token is not repeated: {said}");
    assert!(!said.contains("sk-abcdefghij"), "a token is not repeated: {said}");
}

#[test]
fn a_v2_turn_is_settled_only_when_it_is_taken_quiet_and_idle() {
    assert!(!turn_settled(&None, true, 0), "the session has not said anything yet");
    assert!(!turn_settled(&Some("running".into()), true, 0));
    assert!(!turn_settled(&Some("requires_action".into()), true, 0), "a question is not an end");
    assert!(!turn_settled(&Some("idle".into()), false, 0), "the prompt was not taken yet");
    assert!(!turn_settled(&Some("idle".into()), true, 1), "a dialog is open");
    assert!(turn_settled(&Some("idle".into()), true, 0));
    assert!(!turn_settled(&Some("paused".into()), true, 0), "a state this client does not know is never idle");
}

#[tokio::test]
async fn a_v2_turn_ends_on_the_idle_state_not_the_prompt_answer() {
    let (conn, mut peer) = peer("Mock agent");
    let h = agent("mock");
    let thread = thread();
    let script = tokio::spawn(async move {
        peer.hello().await;
        peer.open("s1").await;
        let p = peer.prompt().await;
        // The answer only says the prompt was inserted: the work goes on.
        accepted(&mut peer, &p["id"]).await;
        peer.update("s1", json!({ "sessionUpdate": "agent_message_chunk", "content": { "type": "text", "text": "still working" } })).await;
        work_state(&mut peer, "s1", "idle", Some("end_turn")).await;
    });
    handshake(&conn, &h).await.expect("the handshake");
    open_session(&conn, &h, &thread, "/tmp").await.expect("a session");
    let (out, events) = turn(&h, &conn, "s1", &thread, "go", false, true).await;
    script.await.expect("the mock agent");

    assert!(said(&out).contains("still working"), "what it said after the answer is in the transcript: {out:?}");
    assert_eq!(ended_on(&out), Some("stop"));
    assert!(events.iter().any(|e| matches!(e, AgentEvent::AgentEnd { .. })));
}

#[tokio::test]
async fn a_v2_turn_that_reports_idle_before_the_prompt_answer_still_ends() {
    let (conn, mut peer) = peer("Mock agent");
    let h = agent("mock");
    let thread = thread();
    let script = tokio::spawn(async move {
        peer.hello().await;
        peer.open("s1").await;
        let p = peer.prompt().await;
        peer.update("s1", json!({ "sessionUpdate": "agent_message_chunk", "content": { "type": "text", "text": "done" } })).await;
        // The spec lets the two arrive in either order.
        work_state(&mut peer, "s1", "idle", Some("end_turn")).await;
        accepted(&mut peer, &p["id"]).await;
    });
    handshake(&conn, &h).await.expect("the handshake");
    open_session(&conn, &h, &thread, "/tmp").await.expect("a session");
    let (out, _events) = turn(&h, &conn, "s1", &thread, "go", false, true).await;
    script.await.expect("the mock agent");

    assert_eq!(ended_on(&out), Some("stop"));
    assert!(!out.iter().any(|m| matches!(m, AgentMessage::Assistant { error_message: Some(_), .. })), "{out:?}");
}

#[tokio::test]
async fn a_v2_turn_that_ends_on_an_error_idle_says_what_failed() {
    let (conn, mut peer) = peer("Mock agent");
    let h = agent("mock");
    let thread = thread();
    let script = tokio::spawn(async move {
        peer.hello().await;
        peer.open("s1").await;
        let p = peer.prompt().await;
        accepted(&mut peer, &p["id"]).await;
        peer.update(
            "s1",
            json!({ "sessionUpdate": "state_update", "state": "idle", "stopReason": "error", "error": { "code": -32603, "message": "the model went away" } }),
        )
        .await;
    });
    handshake(&conn, &h).await.expect("the handshake");
    open_session(&conn, &h, &thread, "/tmp").await.expect("a session");
    let (out, _events) = turn(&h, &conn, "s1", &thread, "go", false, true).await;
    script.await.expect("the mock agent");

    let error = out
        .iter()
        .find_map(|m| match m {
            AgentMessage::Assistant { error_message: Some(e), .. } => Some(e.clone()),
            _ => None,
        })
        .expect("the failure is on the transcript");
    assert!(error.contains("the model went away"), "it says what the agent said: {error}");
    assert_eq!(ended_on(&out), Some("error"));
}

#[tokio::test]
async fn a_v2_turn_keeps_running_through_requires_action() {
    let (conn, mut peer) = peer("Mock agent");
    let h = agent("mock");
    let thread = thread();
    let script = tokio::spawn(async move {
        peer.hello().await;
        peer.open("s1").await;
        let p = peer.prompt().await;
        accepted(&mut peer, &p["id"]).await;
        // The agent is waiting on the user, not finished.
        work_state(&mut peer, "s1", "requires_action", None).await;
        peer.update("s1", json!({ "sessionUpdate": "agent_message_chunk", "content": { "type": "text", "text": "after the question" } })).await;
        work_state(&mut peer, "s1", "idle", Some("end_turn")).await;
    });
    handshake(&conn, &h).await.expect("the handshake");
    open_session(&conn, &h, &thread, "/tmp").await.expect("a session");
    let (out, _events) = turn(&h, &conn, "s1", &thread, "go", false, true).await;
    script.await.expect("the mock agent");

    assert!(said(&out).contains("after the question"), "the turn was still open after `requires_action`: {out:?}");
    assert_eq!(ended_on(&out), Some("stop"));
}

#[tokio::test]
async fn a_v2_state_this_client_does_not_know_does_not_end_the_turn() {
    let (conn, mut peer) = peer("Mock agent");
    let h = agent("mock");
    let thread = thread();
    let script = tokio::spawn(async move {
        peer.hello().await;
        peer.open("s1").await;
        let p = peer.prompt().await;
        accepted(&mut peer, &p["id"]).await;
        // A state this client has no meaning for may mean more work.
        work_state(&mut peer, "s1", "paused", None).await;
        peer.update("s1", json!({ "sessionUpdate": "agent_message_chunk", "content": { "type": "text", "text": "still here" } })).await;
        work_state(&mut peer, "s1", "idle", Some("end_turn")).await;
    });
    handshake(&conn, &h).await.expect("the handshake");
    open_session(&conn, &h, &thread, "/tmp").await.expect("a session");
    let (out, _events) = turn(&h, &conn, "s1", &thread, "go", false, true).await;
    script.await.expect("the mock agent");

    assert!(said(&out).contains("still here"), "an unknown state is not read as idle: {out:?}");
    assert_eq!(ended_on(&out), Some("stop"));
}

#[tokio::test]
async fn a_v2_turn_that_stops_ends_on_the_cancelled_idle() {
    let (conn, mut peer) = peer("Mock agent");
    let h = agent("mock");
    let thread = thread();
    let script = tokio::spawn(async move {
        peer.hello().await;
        peer.open("s1").await;
        let p = peer.prompt().await;
        peer.update("s1", json!({ "sessionUpdate": "agent_message_chunk", "content": { "type": "text", "text": "working" } })).await;
        until_cancel_and_close(&mut peer).await;
        finish(&mut peer, "s1", &p["id"], Some("cancelled")).await;
    });
    handshake(&conn, &h).await.expect("the handshake");
    open_session(&conn, &h, &thread, "/tmp").await.expect("a session");
    let out = stop_the_turn(&h, &conn, "s1", &thread).await;
    script.await.expect("the mock agent");

    assert_eq!(ended_on(&out), Some("aborted"), "a stop is not a failure: {out:?}");
    assert!(!out.iter().any(|m| matches!(m, AgentMessage::Assistant { error_message: Some(_), .. })), "{out:?}");
}

#[tokio::test]
async fn a_v2_idle_without_a_stop_reason_is_a_finished_turn() {
    let (conn, mut peer) = peer("Mock agent");
    let h = agent("mock");
    let thread = thread();
    let script = tokio::spawn(async move {
        peer.hello().await;
        peer.open("s1").await;
        let p = peer.prompt().await;
        peer.update("s1", json!({ "sessionUpdate": "agent_message_chunk", "content": { "type": "text", "text": "done" } })).await;
        // The idle says nothing about why it stopped, which is a turn that finished.
        finish(&mut peer, "s1", &p["id"], None).await;
    });
    handshake(&conn, &h).await.expect("the handshake");
    open_session(&conn, &h, &thread, "/tmp").await.expect("a session");
    let (out, _events) = turn(&h, &conn, "s1", &thread, "go", false, true).await;
    script.await.expect("the mock agent");

    assert_eq!(ended_on(&out), Some("stop"));
    assert!(!out.iter().any(|m| matches!(m, AgentMessage::Assistant { error_message: Some(_), .. })), "{out:?}");
}

#[tokio::test]
async fn a_v1_turn_still_ends_on_the_prompt_answer() {
    let (conn, mut peer) = peer("Mock agent");
    let h = agent("mock");
    let thread = thread();
    let script = tokio::spawn(async move {
        peer.hello_v1(json!({})).await;
        peer.open("s1").await;
        let p = peer.prompt().await;
        peer.update("s1", json!({ "sessionUpdate": "agent_message_chunk", "content": { "type": "text", "text": "done" } })).await;
        // v1 has no state updates: the answer is the whole story.
        peer.answer(&p["id"], json!({ "stopReason": "end_turn" })).await;
    });
    handshake(&conn, &h).await.expect("the handshake");
    open_session(&conn, &h, &thread, "/tmp").await.expect("a session");
    let started = tokio::time::Instant::now();
    let (out, _events) = turn(&h, &conn, "s1", &thread, "go", false, true).await;
    script.await.expect("the mock agent");

    assert_eq!(ended_on(&out), Some("stop"));
    assert!(started.elapsed() < Duration::from_secs(2), "it does not wait for a state update v1 never sends: {:?}", started.elapsed());
}

// ── Phase 5: what the agent says about its work and its session ──────────

/// The finished rows of a turn: tool, text, whether it failed.
fn rows(events: &[AgentEvent]) -> Vec<(String, String, bool)> {
    events
        .iter()
        .filter_map(|e| match e {
            AgentEvent::ToolExecutionEnd { tool_name, result, is_error, .. } => {
                let text = result.content.iter().filter_map(|c| if let MessageContent::Text { text } = c { Some(text.as_str()) } else { None }).collect::<Vec<_>>().join("");
                Some((tool_name.clone(), text, *is_error))
            }
            _ => None,
        })
        .collect()
}

#[test]
fn a_plan_is_read_from_either_versions_update() {
    let items = json!({ "sessionUpdate": "plan_update", "plan": { "type": "items", "planId": "p1", "entries": [
        { "content": "Read", "priority": "high", "status": "completed" },
        { "content": "Fix", "priority": "high", "status": "in_progress" },
        { "content": "Test", "priority": "low", "status": "pending" },
    ] } });
    assert_eq!(plan_change("plan_update", &items), Some(PlanChange::Show { id: "p1".into(), text: "[x] Read\n[~] Fix\n[ ] Test".into() }));
    let md = json!({ "plan": { "type": "markdown", "planId": "p2", "content": "# Steps\n1. go" } });
    assert_eq!(plan_change("plan_update", &md), Some(PlanChange::Show { id: "p2".into(), text: "# Steps\n1. go".into() }));
    let file = json!({ "plan": { "type": "file", "planId": "p3", "uri": "file:///tmp/plan.md" } });
    assert_eq!(plan_change("plan_update", &file), Some(PlanChange::Show { id: "p3".into(), text: "Plan: file:///tmp/plan.md".into() }));
    assert_eq!(plan_change("plan_removed", &json!({ "planId": " p1 " })), Some(PlanChange::Remove { id: "p1".into() }));
    // v1's plan is the whole list every time, under no id.
    assert_eq!(
        plan_change("plan", &json!({ "entries": [{ "content": "Only", "status": "pending" }] })),
        Some(PlanChange::Show { id: "legacy".into(), text: "[ ] Only".into() })
    );
    // A kind of plan it has never heard of, or one with no id, is not drawn.
    assert_eq!(plan_change("plan_update", &json!({ "plan": { "type": "hologram", "planId": "p4" } })), None);
    assert_eq!(plan_change("plan_update", &json!({ "plan": { "type": "markdown", "planId": " ", "content": "x" } })), None);
    assert_eq!(plan_change("plan_removed", &json!({ "planId": "" })), None);
}

#[test]
fn what_the_agent_says_about_its_session_is_kept_and_bounded() {
    let mut info = SessionInfo::default();
    apply_session_info(
        &mut info,
        "available_commands_update",
        &json!({ "availableCommands": [
            { "name": "review", "description": "Review the diff", "input": { "type": "text", "hint": "a branch" } },
            { "name": "  ", "description": "no name" },
            { "name": "init", "description": "Set up" },
        ] }),
    );
    assert_eq!(
        info.commands,
        vec![
            AgentCommand { name: "review".into(), description: "Review the diff".into(), hint: Some("a branch".into()) },
            AgentCommand { name: "init".into(), description: "Set up".into(), hint: None },
        ]
    );
    // The mode is the option that has the `mode` category, or `current_mode_update`.
    apply_session_info(&mut info, "config_option_update", &json!({ "configOptions": [
        { "configId": "model", "name": "Model", "type": "select", "currentValue": "opus", "options": [] },
        { "configId": "mode", "name": "Mode", "category": "mode", "type": "select", "currentValue": "plan", "options": [] },
    ] }));
    assert_eq!(info.mode.as_deref(), Some("plan"));
    assert_eq!(info.config_options.len(), 2);
    apply_session_info(&mut info, "current_mode_update", &json!({ "currentModeId": "code" }));
    assert_eq!(info.mode.as_deref(), Some("code"));
    // A title is set, kept when an update leaves it out, and cleared by `null`.
    apply_session_info(&mut info, "session_info_update", &json!({ "title": "  Fix the build  " }));
    assert_eq!(info.title.as_deref(), Some("Fix the build"));
    apply_session_info(&mut info, "session_info_update", &json!({ "updatedAt": "2026-11-12T00:00:00Z" }));
    assert_eq!(info.title.as_deref(), Some("Fix the build"));
    apply_session_info(&mut info, "session_info_update", &json!({ "title": null }));
    assert_eq!(info.title, None);
    // An agent can't grow it without limit.
    let many: Vec<Value> = (0..500).map(|i| json!({ "name": format!("c{i}"), "description": "x" })).collect();
    apply_session_info(&mut info, "available_commands_update", &json!({ "availableCommands": many }));
    assert_eq!(info.commands.len(), 200);
}

#[test]
fn a_notice_needs_a_title_and_is_bounded() {
    assert_eq!(notice(&json!({ "title": " Rate limited ", "description": "Wait a minute", "severity": "warning" })), Some(("Rate limited".into(), Some("Wait a minute".into()), "warning".into())));
    assert_eq!(notice(&json!({ "title": "Heads up" })), Some(("Heads up".into(), None, "info".into())));
    assert_eq!(notice(&json!({ "title": "  ", "description": "x" })), None);
    assert_eq!(notice(&json!({ "description": "x" })), None);
    let long = notice(&json!({ "title": "t".repeat(5000), "description": "d".repeat(9000) })).unwrap();
    assert_eq!((long.0.chars().count(), long.1.unwrap().chars().count()), (1024, 4096));
    assert_eq!(blocks_text(&[json!({ "type": "text", "text": "a" }), json!({ "type": "image" }), json!({ "type": "text", "text": "b" })]), "a\nb");
}

#[tokio::test]
async fn a_plan_that_changes_and_is_removed_draws_and_then_clears_the_plan() {
    let (conn, mut peer) = peer("Mock agent");
    let h = agent("mock");
    let thread = thread();
    let script = tokio::spawn(async move {
        peer.hello().await;
        peer.open("s1").await;
        let p = peer.prompt().await;
        peer.update("s1", json!({ "sessionUpdate": "plan_update", "plan": { "type": "items", "planId": "p1", "entries": [{ "content": "Read", "priority": "high", "status": "in_progress" }] } })).await;
        peer.update("s1", json!({ "sessionUpdate": "plan_update", "plan": { "type": "items", "planId": "p1", "entries": [{ "content": "Read", "priority": "high", "status": "completed" }] } })).await;
        // Removing a plan that was never on show changes nothing.
        peer.update("s1", json!({ "sessionUpdate": "plan_removed", "planId": "elsewhere" })).await;
        peer.update("s1", json!({ "sessionUpdate": "plan_removed", "planId": "p1" })).await;
        finish(&mut peer, "s1", &p["id"], None).await;
    });
    handshake(&conn, &h).await.expect("the handshake");
    open_session(&conn, &h, &thread, "/tmp").await.expect("a session");
    let (_out, events) = turn(&h, &conn, "s1", &thread, "plan it", false, true).await;
    script.await.expect("the mock agent");
    assert_eq!(
        rows(&events),
        vec![
            ("todo_write".to_string(), "[~] Read".to_string(), false),
            ("todo_write".to_string(), "[x] Read".to_string(), false),
            ("todo_write".to_string(), String::new(), false),
        ],
        "the list as it changed, then an empty one when it was removed"
    );
}

#[tokio::test]
async fn a_compaction_is_one_row_that_ends_with_its_summary_or_its_failure() {
    let (conn, mut peer) = peer("Mock agent");
    let h = agent("mock");
    let thread = thread();
    let script = tokio::spawn(async move {
        peer.hello().await;
        peer.open("s1").await;
        let p = peer.prompt().await;
        peer.update("s1", json!({ "sessionUpdate": "compaction_update", "compactionId": "c1", "status": "in_progress" })).await;
        peer.update("s1", json!({ "sessionUpdate": "compaction_summary_chunk", "compactionId": "c1", "content": { "type": "text", "text": "We fixed " } })).await;
        peer.update("s1", json!({ "sessionUpdate": "compaction_summary_chunk", "compactionId": "c1", "content": { "type": "text", "text": "the build." } })).await;
        peer.update("s1", json!({ "sessionUpdate": "compaction_update", "compactionId": "c1", "status": "completed" })).await;
        peer.update("s1", json!({ "sessionUpdate": "compaction_update", "compactionId": "c2", "status": "failed", "error": "too large" })).await;
        // One left open when the turn ends.
        peer.update("s1", json!({ "sessionUpdate": "compaction_update", "compactionId": "c3", "status": "in_progress" })).await;
        finish(&mut peer, "s1", &p["id"], None).await;
    });
    handshake(&conn, &h).await.expect("the handshake");
    open_session(&conn, &h, &thread, "/tmp").await.expect("a session");
    let (_out, events) = turn(&h, &conn, "s1", &thread, "go on", false, true).await;
    script.await.expect("the mock agent");
    let starts = events.iter().filter(|e| matches!(e, AgentEvent::ToolExecutionStart { tool_name, .. } if tool_name == "compact_context")).count();
    assert_eq!(starts, 3, "one row each, opened once: {events:?}");
    assert_eq!(
        rows(&events),
        vec![
            ("compact_context".to_string(), "We fixed the build.".to_string(), false),
            ("compact_context".to_string(), "too large".to_string(), true),
            ("compact_context".to_string(), "Stopped.".to_string(), true),
        ]
    );
}

#[tokio::test]
async fn a_notice_is_a_row_and_a_title_less_one_is_not() {
    let (conn, mut peer) = peer("Mock agent");
    let h = agent("mock");
    let thread = thread();
    let script = tokio::spawn(async move {
        peer.hello().await;
        peer.open("s1").await;
        let p = peer.prompt().await;
        peer.update("s1", json!({ "sessionUpdate": "notice", "severity": "warning", "title": "Rate limited", "description": "Slowing down for a minute." })).await;
        peer.update("s1", json!({ "sessionUpdate": "notice", "severity": "info", "description": "no title" })).await;
        peer.update("s1", json!({ "sessionUpdate": "notice", "severity": "error", "title": "Out of credit" })).await;
        finish(&mut peer, "s1", &p["id"], None).await;
    });
    handshake(&conn, &h).await.expect("the handshake");
    open_session(&conn, &h, &thread, "/tmp").await.expect("a session");
    let (_out, events) = turn(&h, &conn, "s1", &thread, "go", false, true).await;
    script.await.expect("the mock agent");
    assert_eq!(
        rows(&events),
        vec![("notice".to_string(), "Slowing down for a minute.".to_string(), false), ("notice".to_string(), "Out of credit".to_string(), true)]
    );
}

#[tokio::test]
async fn how_full_the_agents_context_is_is_kept_for_the_meter_and_a_new_session_forgets_it() {
    let (conn, mut peer) = peer("Mock agent");
    let h = agent("mock");
    let thread = thread();
    let script = tokio::spawn(async move {
        peer.hello().await;
        peer.open("s1").await;
        let p = peer.prompt().await;
        peer.update("s1", json!({ "sessionUpdate": "usage_update", "used": 52_000, "size": 200_000, "cost": { "amount": 0.12, "currency": "USD" } })).await;
        // A window of nothing is not a window: the figure before it stands.
        peer.update("s1", json!({ "sessionUpdate": "usage_update", "used": 1, "size": 0 })).await;
        finish(&mut peer, "s1", &p["id"], None).await;
    });
    handshake(&conn, &h).await.expect("the handshake");
    open_session(&conn, &h, &thread, "/tmp").await.expect("a session");
    assert_eq!(context_usage(&thread), None, "nothing is known before the agent says");
    let (_out, _events) = turn(&h, &conn, "s1", &thread, "go", false, true).await;
    script.await.expect("the mock agent");
    assert_eq!(context_usage(&thread), Some((52_000, 200_000)));

    // The same thread on a new session, which has seen nothing, starts empty.
    let (conn2, mut peer2) = self::peer("Mock agent");
    let again = tokio::spawn(async move {
        peer2.hello().await;
        peer2.open("s2").await
    });
    handshake(&conn2, &h).await.expect("the handshake");
    open_session(&conn2, &h, &thread, "/tmp").await.expect("a new session");
    again.await.expect("the mock agent");
    assert_eq!(context_usage(&thread), None);
}

#[tokio::test]
async fn the_commands_mode_and_title_an_agent_reports_are_there_to_read() {
    let (conn, mut peer) = peer("Mock agent");
    let h = agent("mock");
    let thread = thread();
    let script = tokio::spawn(async move {
        peer.hello().await;
        peer.open("s1").await;
        let p = peer.prompt().await;
        peer.update("s1", json!({ "sessionUpdate": "available_commands_update", "availableCommands": [{ "name": "review", "description": "Review the diff" }] })).await;
        peer.update("s1", json!({ "sessionUpdate": "config_option_update", "configOptions": [{ "configId": "mode", "name": "Mode", "category": "mode", "type": "select", "currentValue": "plan", "options": [] }] })).await;
        peer.update("s1", json!({ "sessionUpdate": "session_info_update", "title": "Fix the build" })).await;
        finish(&mut peer, "s1", &p["id"], None).await;
    });
    handshake(&conn, &h).await.expect("the handshake");
    open_session(&conn, &h, &thread, "/tmp").await.expect("a session");
    let (out, events) = turn(&h, &conn, "s1", &thread, "go", false, true).await;
    script.await.expect("the mock agent");
    let info = conn.info.lock().unwrap().clone();
    assert_eq!(info.commands.len(), 1);
    assert_eq!((info.mode.as_deref(), info.title.as_deref()), (Some("plan"), Some("Fix the build")));
    // None of it is a row in the chat.
    assert!(rows(&events).is_empty(), "{events:?}");
    assert_eq!(out.len(), 1, "only the user's message: {out:?}");
}

// ── Phases 6 and 7: Vivianne's tools in the agent, and what it is told ───

/// A v2 agent that advertises MCP over ACP and HTTP servers.
fn v2_mcp_caps(acp: bool, http: bool) -> Value {
    let mut mcp = serde_json::Map::new();
    if acp {
        mcp.insert("acp".into(), json!({}));
    }
    if http {
        mcp.insert("http".into(), json!({}));
    }
    json!({ "session": { "mcp": mcp } })
}

#[tokio::test]
async fn viviannes_tools_are_offered_the_way_the_agent_can_take_them() {
    use crate::agent::harness_mcp::SERVER_NAME;
    let h = agent("mock");
    // Over ACP when it takes that: nothing listens.
    let (conn, mut peer) = peer("Mock agent");
    let script = tokio::spawn(async move { peer.hello_version(2, v2_mcp_caps(true, true), json!([])).await });
    handshake(&conn, &h).await.expect("the handshake");
    script.await.unwrap();
    assert_eq!(own_mcp_server(&conn, true).await, Some(json!({ "type": "acp", "name": SERVER_NAME, "serverId": SERVER_NAME })));
    assert!(conn.mcp_http.lock().await.is_none(), "no endpoint is opened when the agent can be reached over its own connection");

    // A local endpoint behind a token when it only takes HTTP servers.
    let (conn, mut peer) = self::peer("Mock agent");
    let script = tokio::spawn(async move { peer.hello_version(2, v2_mcp_caps(false, true), json!([])).await });
    handshake(&conn, &h).await.expect("the handshake");
    script.await.unwrap();
    let entry = own_mcp_server(&conn, true).await.expect("an HTTP entry");
    assert_eq!((entry["type"].clone(), entry["name"].clone()), (json!("http"), json!(SERVER_NAME)));
    let url = entry["url"].as_str().unwrap();
    assert!(url.starts_with("http://127.0.0.1:") && url.ends_with("/mcp"), "loopback only: {url}");
    let bearer = entry["headers"][0]["value"].as_str().unwrap();
    assert!(bearer.starts_with("Bearer ") && bearer.len() > 40, "a token that can't be guessed");
    // Asked again, the same endpoint: one per connection.
    assert_eq!(own_mcp_server(&conn, true).await, Some(entry));

    // Nothing when the agent takes neither, or when there is no app to run a tool in.
    let (conn, mut peer) = self::peer("Mock agent");
    let script = tokio::spawn(async move { peer.hello_version(2, v2_mcp_caps(false, false), json!([])).await });
    handshake(&conn, &h).await.expect("the handshake");
    script.await.unwrap();
    assert_eq!(own_mcp_server(&conn, true).await, None);
    assert_eq!(own_mcp_server(&conn, false).await, None);
}

#[tokio::test]
async fn an_agent_can_use_the_server_over_its_own_connection() {
    let (conn, mut peer) = peer("Mock agent");
    let h = agent("mock");
    let script = tokio::spawn(async move {
        peer.hello_version(2, v2_mcp_caps(true, false), json!([])).await;
        let (connected, _) = peer.call(70, "mcp/connect", json!({ "serverId": "vivianne" })).await;
        let id = connected["connectionId"].as_str().expect("a connection id").to_string();
        let (hello, _) = peer.call(71, "mcp/message", json!({ "connectionId": id, "method": "initialize", "params": { "protocolVersion": "2025-06-18" } })).await;
        let (listed, _) = peer.call(72, "mcp/message", json!({ "connectionId": id, "method": "tools/list" })).await;
        // No turn is running, so a call is refused as a tool error, not run.
        let (called, _) = peer.call(73, "mcp/message", json!({ "connectionId": id, "method": "tools/call", "params": { "name": "recall", "arguments": { "query": "x" } } })).await;
        // A tool that is not offered is a protocol error.
        peer.send_request(74, "mcp/message", json!({ "connectionId": id, "method": "tools/call", "params": { "name": "vault", "arguments": {} } })).await;
        let mut vault = None;
        while vault.is_none() {
            let m = peer.recv().await;
            if m["id"] == json!(74) {
                vault = Some(m);
            }
        }
        peer.send_request(75, "mcp/connect", json!({ "serverId": "someone-else" })).await;
        let mut other = None;
        while other.is_none() {
            let m = peer.recv().await;
            if m["id"] == json!(75) {
                other = Some(m);
            }
        }
        let (gone, _) = peer.call(76, "mcp/disconnect", json!({ "connectionId": id })).await;
        (hello, listed, called, vault.unwrap(), other.unwrap(), gone)
    });
    handshake(&conn, &h).await.expect("the handshake");
    let (hello, listed, called, vault, other, gone) = script.await.expect("the mock agent");
    assert_eq!(hello["serverInfo"]["name"], "vivianne");
    assert!(listed["tools"].as_array().unwrap().iter().any(|t| t["name"] == "recall"));
    assert_eq!(called["isError"], true);
    assert_eq!(vault["error"]["code"], -32602, "{vault}");
    assert_eq!(other["error"]["code"], -32602, "an unknown server is refused: {other}");
    assert_eq!(gone, json!({}));
}

#[tokio::test]
async fn the_agent_is_told_where_it_runs_once_and_again_only_when_that_changes() {
    let (conn, mut peer) = peer("Mock agent");
    let h = agent("mock");
    let thread = thread();
    // The text blocks of each prompt the agent was sent.
    let script = tokio::spawn(async move {
        peer.hello().await;
        peer.open("s1").await;
        let mut prompts = Vec::new();
        for _ in 0..5 {
            let p = peer.prompt().await;
            prompts.push(p["params"]["prompt"].as_array().unwrap().iter().filter_map(|b| b["text"].as_str().map(String::from)).collect::<Vec<_>>());
            finish(&mut peer, "s1", &p["id"], None).await;
        }
        prompts
    });
    handshake(&conn, &h).await.expect("the handshake");
    open_session(&conn, &h, &thread, "/tmp").await.expect("a session");
    let said = |plan: bool, text: &'static str| {
        let (h, conn, thread) = (h.clone(), conn.clone(), thread.clone());
        async move { turn_in(&h, &conn, "s1", &thread, text, false, plan, true).await }
    };
    said(false, "one").await; // nothing to say yet
    said(true, "two").await; // plan mode is on: told
    said(true, "three").await; // still on: not told again
    said(false, "four").await; // off: told it is over
    // A completed compaction takes what it was told with it: plan mode again, and it is told again.
    *conn.told.lock().unwrap() = None;
    said(true, "/review").await; // a command of its own: nothing goes before it
    let prompts = script.await.expect("the mock agent");

    assert_eq!(prompts[0], vec!["one"], "{prompts:?}");
    assert_eq!(prompts[1].len(), 2, "{prompts:?}");
    assert!(prompts[1][0].contains("Plan mode is on") && prompts[1][1] == "two", "{prompts:?}");
    assert_eq!(prompts[2], vec!["three"], "said once: {prompts:?}");
    assert!(prompts[3][0].contains("Plan mode is off") && prompts[3][1] == "four", "{prompts:?}");
    assert_eq!(prompts[4], vec!["/review"], "a command stays first, alone: {prompts:?}");
}

#[tokio::test]
async fn a_finished_compaction_makes_the_agent_be_told_again() {
    let (conn, mut peer) = peer("Mock agent");
    let h = agent("mock");
    let thread = thread();
    let script = tokio::spawn(async move {
        peer.hello().await;
        peer.open("s1").await;
        let p = peer.prompt().await;
        let first = p["params"]["prompt"].as_array().unwrap().len();
        peer.update("s1", json!({ "sessionUpdate": "compaction_update", "compactionId": "c1", "status": "completed" })).await;
        finish(&mut peer, "s1", &p["id"], None).await;
        let p = peer.prompt().await;
        let second = p["params"]["prompt"].as_array().unwrap().len();
        finish(&mut peer, "s1", &p["id"], None).await;
        (first, second)
    });
    handshake(&conn, &h).await.expect("the handshake");
    open_session(&conn, &h, &thread, "/tmp").await.expect("a session");
    turn_in(&h, &conn, "s1", &thread, "go", false, true, true).await;
    turn_in(&h, &conn, "s1", &thread, "go on", false, true, true).await;
    // Told on the first turn (plan mode), the compaction cleared it, so told again on the second.
    assert_eq!(script.await.expect("the mock agent"), (2, 2));
}

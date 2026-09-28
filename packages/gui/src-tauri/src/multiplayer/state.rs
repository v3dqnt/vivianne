//! Connection lifecycle: one iroh `Endpoint` per app instance, always
//! listening (so either side can be the one who's dialed), one active peer
//! at a time. Holds the mirrored session messages and chat log a paired
//! peer has sent so far.

use std::sync::Arc;
use std::time::Duration;

use iroh::endpoint::{Connection, RecvStream, SendStream};
use iroh::{Endpoint, EndpointId};
use tauri::{AppHandle, Emitter};
use tokio::sync::{mpsc, RwLock};

use crate::agent::types::AgentMessage;
use super::protocol::{self, WireMessage, ALPN, PROTOCOL_VERSION};

/// How long a single frame write may take before the connection is
/// considered dead. Without this, a stalled peer (sleeping laptop, network
/// gone) left `write_all` on the QUIC stream hanging indefinitely — and
/// because broadcasting used to be awaited inline in the same loop that
/// forwards local agent events to the UI, a stuck remote write froze local
/// token streaming too. The timeout lives on the dedicated writer task
/// (see `spawn_writer`), never on the caller.
const WRITE_TIMEOUT: Duration = Duration::from_secs(5);

/// How long `connect()` waits for the peer to answer before giving up —
/// dialing an unreachable/offline peer otherwise hangs forever with no way
/// for the UI to tell "still trying" from "never going to work."
const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);

/// Outbound messages queued but not yet written. Bounded and non-blocking
/// on the send side (`try_send`): a slow peer fills this and new broadcasts
/// get dropped rather than backing up whatever's producing them (the agent
/// turn-completion path), which is the same "coalesce/drop under
/// backpressure" contract the terminal output pump and streaming-shell
/// batcher already use elsewhere in this codebase.
const OUTBOUND_QUEUE_CAPACITY: usize = 128;

/// Mirrored history is bounded, not because old entries are unsafe to
/// keep, but because an unbounded `Vec` fed by a live socket is exactly the
/// shape of bug that turns a long pairing session into unbounded RSS growth
/// (see `docs/multiplayer-update-plan.md`'s "shared sessions" section for
/// what a real, persisted history would look like instead — this is still
/// phase 1, in-memory only).
const MAX_MIRRORED_ENTRIES: usize = 500;

/// Hard cap on a peer-supplied display name — it's rendered directly in the
/// UI ("Paired with …") with no length limit otherwise, so a hostile or
/// buggy peer could send an arbitrarily long string.
const MAX_DISPLAY_NAME_CHARS: usize = 64;

#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum ConnectionStatus {
    Disconnected,
    Connecting,
    Connected { peer_id: String, peer_display_name: Option<String> },
    Failed { reason: String },
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatEntry {
    pub from_peer: bool,
    pub text: String,
    pub timestamp: u64,
}

pub struct MultiplayerState {
    pub my_id: String,
    endpoint: Endpoint,
    status: RwLock<ConnectionStatus>,
    /// What this machine calls itself in a `Hello` — editable via
    /// `cmd_multiplayer_set_display_name`, defaults to the OS username so
    /// there's always something reasonable to show without requiring the
    /// separate account system to be signed in.
    display_name: RwLock<Option<String>>,
    /// The current connection's outbound queue — `Some` only while a
    /// connection's writer task is alive. Broadcasting never touches the
    /// network directly; it hands a message to this channel and returns
    /// immediately (see `WRITE_TIMEOUT`'s doc comment for why).
    outbound_tx: RwLock<Option<mpsc::Sender<WireMessage>>>,
    peer_messages: RwLock<Vec<AgentMessage>>,
    chat_log: RwLock<Vec<ChatEntry>>,
    /// Bumped on every new connection (incoming or outgoing) so a read/write
    /// loop from a since-replaced connection knows to stop touching shared
    /// state instead of clobbering a newer connection's.
    generation: std::sync::atomic::AtomicU64,
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() as u64
}

fn default_display_name() -> Option<String> {
    // VIV_DISPLAY_NAME names a second instance on the same machine (a test
    // partner), which would otherwise introduce itself with the same OS
    // username as the first.
    std::env::var("VIV_DISPLAY_NAME")
        .or_else(|_| std::env::var("USERNAME"))
        .or_else(|_| std::env::var("USER"))
        .ok()
}

fn cap_display_name(name: Option<String>) -> Option<String> {
    name.map(|n| n.chars().take(MAX_DISPLAY_NAME_CHARS).collect::<String>()).filter(|n| !n.is_empty())
}

fn push_bounded<T>(vec: &mut Vec<T>, item: T) {
    vec.push(item);
    if vec.len() > MAX_MIRRORED_ENTRIES {
        let excess = vec.len() - MAX_MIRRORED_ENTRIES;
        vec.drain(..excess);
    }
}

impl MultiplayerState {
    pub async fn new() -> Result<Self, String> {
        let secret_key = super::identity::load_or_create().await?;
        let my_id = secret_key.public().to_string();
        let endpoint = Endpoint::builder(iroh::endpoint::presets::N0)
            .secret_key(secret_key)
            .alpns(vec![ALPN.to_vec()])
            .bind()
            .await
            .map_err(|e| format!("failed to start multiplayer endpoint: {e}"))?;
        Ok(Self {
            my_id,
            endpoint,
            status: RwLock::new(ConnectionStatus::Disconnected),
            display_name: RwLock::new(default_display_name()),
            outbound_tx: RwLock::new(None),
            peer_messages: RwLock::new(Vec::new()),
            chat_log: RwLock::new(Vec::new()),
            generation: std::sync::atomic::AtomicU64::new(0),
        })
    }

    pub async fn status(&self) -> ConnectionStatus {
        self.status.read().await.clone()
    }

    pub async fn display_name(&self) -> Option<String> {
        self.display_name.read().await.clone()
    }

    pub async fn set_display_name(&self, name: Option<String>) {
        *self.display_name.write().await = cap_display_name(name);
    }

    pub async fn peer_messages(&self) -> Vec<AgentMessage> {
        self.peer_messages.read().await.clone()
    }

    pub async fn chat_log(&self) -> Vec<ChatEntry> {
        self.chat_log.read().await.clone()
    }

    /// Runs forever on its own task (spawned once at startup) — either side
    /// can be the one who gets dialed, so this has to be listening
    /// regardless of whether this instance ever calls `connect`. Every
    /// inbound connection is checked against
    /// `agent::projects::is_authorized_peer` *before* any stream is opened
    /// or a single protocol byte is exchanged — an unrecognized peer is
    /// closed immediately. Knowing this machine's pairing code was never
    /// meant to be equivalent to being let in; only a peer some multiplayer
    /// project actually named gets that.
    pub async fn run_accept_loop(self: Arc<Self>, app: AppHandle) {
        loop {
            let Some(incoming) = self.endpoint.accept().await else { break };
            let app = app.clone();
            let this = self.clone();
            tokio::spawn(async move {
                let conn = match incoming.await {
                    Ok(c) => c,
                    Err(e) => {
                        log::warn!("[multiplayer] incoming connection failed: {e}");
                        return;
                    }
                };
                let peer_id = conn.remote_id().to_string();
                if !crate::agent::projects::is_authorized_peer(&peer_id) {
                    log::warn!("[multiplayer] rejected connection from unauthorized peer {peer_id}");
                    conn.close(1u32.into(), b"peer not authorized for any multiplayer project");
                    return;
                }
                this.adopt_connection(conn, app, false).await;
            });
        }
    }

    pub async fn connect(self: &Arc<Self>, peer_id_str: &str, app: AppHandle) -> Result<(), String> {
        let peer_id: EndpointId = peer_id_str.trim().parse().map_err(|_| "That doesn't look like a valid pairing code.".to_string())?;
        *self.status.write().await = ConnectionStatus::Connecting;
        let _ = app.emit("multiplayer_status", self.status().await);

        let conn = match tokio::time::timeout(CONNECT_TIMEOUT, self.endpoint.connect(peer_id, ALPN)).await {
            Ok(Ok(c)) => c,
            Ok(Err(e)) => {
                let reason = format!("Couldn't reach that peer: {e}");
                *self.status.write().await = ConnectionStatus::Failed { reason: reason.clone() };
                let _ = app.emit("multiplayer_status", self.status().await);
                return Err(reason);
            }
            Err(_) => {
                let reason = "Timed out trying to reach that peer.".to_string();
                *self.status.write().await = ConnectionStatus::Failed { reason: reason.clone() };
                let _ = app.emit("multiplayer_status", self.status().await);
                return Err(reason);
            }
        };
        self.clone().adopt_connection(conn, app, true).await;
        Ok(())
    }

    /// Shared by both the dialer and the accepter once a `Connection`
    /// exists (the accepter has already checked `is_authorized_peer` — see
    /// `run_accept_loop`): the dialer opens the one stream this protocol
    /// uses, the accepter waits for it, both exchange `Hello`, then hand
    /// off to the read loop and start a dedicated writer task for outbound
    /// frames.
    async fn adopt_connection(self: Arc<Self>, conn: Connection, app: AppHandle, is_dialer: bool) {
        let generation = self.generation.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
        let peer_id = conn.remote_id().to_string();

        let streams = if is_dialer { conn.open_bi().await } else { conn.accept_bi().await };
        let (mut send, mut recv) = match streams {
            Ok(s) => s,
            Err(e) => {
                log::warn!("[multiplayer] failed to establish stream with {peer_id}: {e}");
                if generation == self.generation.load(std::sync::atomic::Ordering::SeqCst) {
                    *self.status.write().await = ConnectionStatus::Disconnected;
                    let _ = app.emit("multiplayer_status", self.status().await);
                }
                return;
            }
        };

        let my_display_name = self.display_name().await;
        if protocol::write_frame(&mut send, &WireMessage::Hello { protocol_version: PROTOCOL_VERSION, display_name: my_display_name }).await.is_err() {
            return;
        }

        let peer_display_name = match protocol::read_frame(&mut recv).await {
            Ok(Some(WireMessage::Hello { protocol_version, display_name })) => {
                if protocol_version != PROTOCOL_VERSION {
                    log::warn!("[multiplayer] peer {peer_id} speaks protocol v{protocol_version}, we speak v{PROTOCOL_VERSION} — refusing");
                    if generation == self.generation.load(std::sync::atomic::Ordering::SeqCst) {
                        *self.status.write().await = ConnectionStatus::Failed { reason: "Your pair is running an incompatible version of Vivianne.".to_string() };
                        let _ = app.emit("multiplayer_status", self.status().await);
                    }
                    return;
                }
                cap_display_name(display_name)
            }
            _ => None,
        };

        if generation != self.generation.load(std::sync::atomic::Ordering::SeqCst) {
            return; // superseded by a newer connection while we were handshaking
        }

        let (tx, rx) = mpsc::channel(OUTBOUND_QUEUE_CAPACITY);
        *self.outbound_tx.write().await = Some(tx);
        self.clone().spawn_writer(send, rx, generation, app.clone());

        *self.status.write().await = ConnectionStatus::Connected { peer_id: peer_id.clone(), peer_display_name };
        let _ = app.emit("multiplayer_status", self.status().await);
        tokio::spawn(super::collab::on_connected(app.clone(), peer_id.clone()));

        self.run_read_loop(recv, generation, app, peer_id).await;
    }

    /// Owns the `SendStream` for this connection's lifetime, draining
    /// `rx` and writing each frame with `WRITE_TIMEOUT`. This is the only
    /// place that ever touches the network on the send side — everything
    /// else (`broadcast_session_message`, `send_chat`) just queues and
    /// returns, so a stalled peer can only ever stall this one task, never
    /// the agent-event forwarding loop or a caller awaiting `send_chat`.
    fn spawn_writer(self: Arc<Self>, mut send: SendStream, mut rx: mpsc::Receiver<WireMessage>, generation: u64, app: AppHandle) {
        tokio::spawn(async move {
            while let Some(msg) = rx.recv().await {
                match tokio::time::timeout(WRITE_TIMEOUT, protocol::write_frame(&mut send, &msg)).await {
                    Ok(Ok(())) => {}
                    Ok(Err(e)) => {
                        log::warn!("[multiplayer] write failed, dropping connection: {e}");
                        break;
                    }
                    Err(_) => {
                        log::warn!("[multiplayer] write timed out after {:?}, dropping connection", WRITE_TIMEOUT);
                        break;
                    }
                }
            }
            if generation == self.generation.load(std::sync::atomic::Ordering::SeqCst) {
                *self.outbound_tx.write().await = None;
                *self.status.write().await = ConnectionStatus::Disconnected;
                let _ = app.emit("multiplayer_status", self.status().await);
            }
        });
    }

    async fn run_read_loop(self: Arc<Self>, mut recv: RecvStream, generation: u64, app: AppHandle, peer_id: String) {
        loop {
            match protocol::read_frame(&mut recv).await {
                Ok(Some(WireMessage::SessionMessage { message })) => {
                    let mut msgs = self.peer_messages.write().await;
                    push_bounded(&mut msgs, (*message).clone());
                    drop(msgs);
                    let _ = app.emit("multiplayer_peer_message", &*message);
                }
                Ok(Some(WireMessage::Chat { text })) => {
                    let entry = ChatEntry { from_peer: true, text, timestamp: now_ms() };
                    let mut log = self.chat_log.write().await;
                    push_bounded(&mut log, entry.clone());
                    drop(log);
                    let _ = app.emit("multiplayer_chat", &entry);
                }
                Ok(Some(WireMessage::Hello { .. })) => {} // only expected once, at handshake
                Ok(Some(msg @ (WireMessage::Activity { .. } | WireMessage::Usage { .. } | WireMessage::GitBundle { .. } | WireMessage::GitHead { .. }))) => {
                    // Off the read loop: merging commits runs git, which
                    // must not stall the next frame.
                    tokio::spawn(super::collab::on_message(app.clone(), peer_id.clone(), msg));
                }
                Ok(None) => break,
                Err(e) => {
                    log::warn!("[multiplayer] connection lost: {e}");
                    break;
                }
            }
        }
        if generation == self.generation.load(std::sync::atomic::Ordering::SeqCst) {
            *self.outbound_tx.write().await = None;
            *self.status.write().await = ConnectionStatus::Disconnected;
            let _ = app.emit("multiplayer_status", self.status().await);
        }
    }

    pub async fn disconnect(&self) {
        self.generation.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        *self.outbound_tx.write().await = None;
        *self.status.write().await = ConnectionStatus::Disconnected;
    }

    /// Best-effort, non-blocking — called from the agent turn-completion
    /// path with each newly produced message. Queues onto the writer's
    /// channel and returns immediately; never touches the network directly
    /// (see `spawn_writer`). A full queue (peer not draining fast enough)
    /// or no connection at all both just drop the message with a log line
    /// rather than propagating backpressure to the caller — the same
    /// "silently do nothing until there's a sink" shape as
    /// `coding_agent::streaming::send_batch`, extended with "and don't
    /// block if the sink is slow."
    pub async fn broadcast_session_message(&self, message: &AgentMessage) {
        let Some(tx) = self.outbound_tx.read().await.clone() else { return };
        if tx.try_send(WireMessage::SessionMessage { message: Box::new(message.clone()) }).is_err() {
            log::warn!("[multiplayer] outbound queue full or closed, dropping a session-message broadcast");
        }
    }

    /// Queue any frame for the peer. Non-blocking, like every send here;
    /// `false` when there's no connection or the queue is full.
    pub async fn send(&self, msg: WireMessage) -> bool {
        let Some(tx) = self.outbound_tx.read().await.clone() else { return false };
        tx.try_send(msg).is_ok()
    }

    pub async fn send_chat(&self, text: String) -> Result<(), String> {
        let tx = self.outbound_tx.read().await.clone().ok_or_else(|| "Not connected to a peer.".to_string())?;
        tx.try_send(WireMessage::Chat { text: text.clone() }).map_err(|_| "Connection is busy, try again.".to_string())?;
        let mut log = self.chat_log.write().await;
        push_bounded(&mut log, ChatEntry { from_peer: false, text, timestamp: now_ms() });
        Ok(())
    }
}

#[cfg(test)]
mod live_tests {
    use super::*;

    /// Two endpoints on this machine reach each other by id and trade the
    /// protocol's Hello frames — the path two Vivianne windows take.
    /// `cargo test --lib two_endpoints_pair -- --ignored --nocapture`
    #[tokio::test(flavor = "multi_thread")]
    #[ignore]
    async fn two_endpoints_pair() {
        let make = || async {
            Endpoint::builder(iroh::endpoint::presets::N0)
                .secret_key(iroh::SecretKey::generate())
                .alpns(vec![ALPN.to_vec()])
                .bind()
                .await
                .unwrap()
        };
        let (a, b) = (make().await, make().await);
        let b_id = b.id();
        let server = tokio::spawn(async move {
            let conn = b.accept().await.unwrap().await.unwrap();
            let (mut send, mut recv) = conn.accept_bi().await.unwrap();
            let hello = protocol::read_frame(&mut recv).await.unwrap();
            protocol::write_frame(&mut send, &WireMessage::Hello { protocol_version: PROTOCOL_VERSION, display_name: Some("B".into()) }).await.unwrap();
            send.finish().unwrap();
            let _ = conn.closed().await;
            hello
        });
        let started = std::time::Instant::now();
        let conn = tokio::time::timeout(CONNECT_TIMEOUT, a.connect(b_id, ALPN)).await.expect("connect timed out").expect("connect failed");
        println!("connected in {:?}", started.elapsed());
        let (mut send, mut recv) = conn.open_bi().await.unwrap();
        protocol::write_frame(&mut send, &WireMessage::Hello { protocol_version: PROTOCOL_VERSION, display_name: Some("A".into()) }).await.unwrap();
        let reply = protocol::read_frame(&mut recv).await.unwrap();
        println!("handshake done in {:?}: {reply:?}", started.elapsed());
        conn.close(0u32.into(), b"done");
        let got = server.await.unwrap();
        assert!(matches!(got, Some(WireMessage::Hello { .. })));
        assert!(matches!(reply, Some(WireMessage::Hello { .. })));
    }
}

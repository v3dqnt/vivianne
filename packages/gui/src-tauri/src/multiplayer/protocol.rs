//! The wire protocol for a paired connection: one ALPN, one long-lived
//! bidirectional QUIC stream per session, carrying length-prefixed JSON
//! frames. No CRDT yet (see the multiplayer plan doc's phased build order —
//! this is phase 1, transport + a first cut at shared sessions); each side
//! just appends its own new turn messages and forwards them as they happen.

use crate::agent::types::AgentMessage;
use serde::{Deserialize, Serialize};

/// Bumped if the frame shape ever changes incompatibly — sent in `Hello` so
/// two mismatched app versions fail with a clear reason instead of a
/// confusing deserialize error deep in the stream.
pub const PROTOCOL_VERSION: u32 = 1;

pub const ALPN: &[u8] = b"vivianne/multiplayer/1";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum WireMessage {
    /// First frame either side sends after the connection opens.
    Hello { protocol_version: u32, display_name: Option<String> },
    /// One message this peer's agent just produced (user turn, assistant
    /// reply, tool call/result) — broadcast as it's appended locally so the
    /// other side's session mirror stays live, not just synced on request.
    SessionMessage { message: Box<AgentMessage> },
    /// Human chat, kept on the same connection rather than a second one —
    /// there's only ever one peer, no reason to multiplex differently.
    Chat { text: String },
}

/// Real frames (chat, a turn's messages, even a base64 screenshot) are
/// firmly in the low-single-digit-MB range at most — this is deliberately
/// not 16MB: a smaller cap shrinks both how much a hostile/buggy peer can
/// force allocated per frame and how much one oversized legitimate message
/// can hold the writer's `WRITE_TIMEOUT` window hostage.
const MAX_FRAME_BYTES: usize = 4 * 1024 * 1024;

pub async fn write_frame(stream: &mut iroh::endpoint::SendStream, msg: &WireMessage) -> Result<(), String> {
    let body = serde_json::to_vec(msg).map_err(|e| format!("failed to encode multiplayer frame: {e}"))?;
    if body.len() > MAX_FRAME_BYTES {
        return Err(format!("multiplayer frame too large ({} bytes)", body.len()));
    }
    let len = (body.len() as u32).to_be_bytes();
    stream.write_all(&len).await.map_err(|e| format!("multiplayer write failed: {e}"))?;
    stream.write_all(&body).await.map_err(|e| format!("multiplayer write failed: {e}"))?;
    Ok(())
}

/// `None` on a clean stream close (peer disconnected) — not an error, the
/// caller's read loop just ends.
///
/// Reads the body incrementally in bounded chunks rather than trusting the
/// peer-supplied length prefix for one big up-front `vec![0u8; len]` — a
/// peer can claim any length up to `MAX_FRAME_BYTES` in a 4-byte prefix and
/// then send nothing, so allocating for the claim before a single body byte
/// arrives is a cheap way to force large allocations. Growing the buffer as
/// bytes actually arrive means memory used tracks bytes actually received.
pub async fn read_frame(stream: &mut iroh::endpoint::RecvStream) -> Result<Option<WireMessage>, String> {
    let mut len_buf = [0u8; 4];
    match stream.read_exact(&mut len_buf).await {
        Ok(()) => {}
        Err(iroh::endpoint::ReadExactError::FinishedEarly(0)) => return Ok(None),
        Err(e) => return Err(format!("multiplayer read failed: {e}")),
    }
    let len = u32::from_be_bytes(len_buf) as usize;
    if len > MAX_FRAME_BYTES {
        return Err(format!("peer sent an oversized multiplayer frame ({len} bytes)"));
    }

    const CHUNK: usize = 64 * 1024;
    let mut body = Vec::with_capacity(len.min(CHUNK));
    let mut remaining = len;
    let mut chunk_buf = [0u8; CHUNK];
    while remaining > 0 {
        let to_read = remaining.min(CHUNK);
        stream.read_exact(&mut chunk_buf[..to_read]).await.map_err(|e| format!("multiplayer read failed: {e}"))?;
        body.extend_from_slice(&chunk_buf[..to_read]);
        remaining -= to_read;
    }
    serde_json::from_slice(&body).map(Some).map_err(|e| format!("failed to decode multiplayer frame: {e}"))
}

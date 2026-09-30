//! This machine's stable multiplayer identity — a persisted iroh
//! `SecretKey` whose public half (the `EndpointId`) is the "pairing code"
//! shown to the user and shared with a peer. Generated once; reused across
//! restarts so a paired peer's saved code keeps working.
//!
//! Not stored in the OS keychain like account sessions or provider keys:
//! this key authenticates a *machine*, not a person, and its public half is
//! meant to be shared out loud (read over a call, pasted in a chat) — a
//! plain file next to the rest of `~/.viv/agent` is the right trust level,
//! matching how `session_id`/other local app data is stored, not the
//! higher-sensitivity keychain path.

use iroh::SecretKey;

fn key_path() -> Result<std::path::PathBuf, String> {
    Ok(crate::agent::paths::viv_agent_dir()?.join("multiplayer_key"))
}

pub async fn load_or_create() -> Result<SecretKey, String> {
    let path = key_path()?;
    if let Ok(hex) = tokio::fs::read_to_string(&path).await {
        if let Ok(bytes) = hex_to_bytes32(hex.trim()) {
            return Ok(SecretKey::from_bytes(&bytes));
        }
        log::warn!("[multiplayer] stored identity at {} is unreadable, generating a new one", path.display());
    }
    let key = SecretKey::generate();
    let hex = bytes_to_hex(&key.to_bytes());
    if let Err(e) = tokio::fs::write(&path, hex).await {
        log::warn!("[multiplayer] failed to persist identity, a new one will be generated next launch: {e}");
    }
    Ok(key)
}

fn bytes_to_hex(bytes: &[u8; 32]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn hex_to_bytes32(hex: &str) -> Result<[u8; 32], String> {
    if hex.len() != 64 {
        return Err("wrong length".to_string());
    }
    let mut out = [0u8; 32];
    for i in 0..32 {
        out[i] = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).map_err(|e| e.to_string())?;
    }
    Ok(out)
}

/// The pairing code of the profile whose data lives in `profile_home`,
/// creating that profile's identity if it has none yet — so a test partner's
/// code is known (and can be authorized) before it is ever launched.
pub fn id_for_profile_home(profile_home: &std::path::Path) -> Result<String, String> {
    let dir = profile_home.join("agent");
    std::fs::create_dir_all(&dir).map_err(|e| format!("couldn't create {}: {e}", dir.display()))?;
    let path = dir.join("multiplayer_key");
    let key = match std::fs::read_to_string(&path).ok().and_then(|h| hex_to_bytes32(h.trim()).ok()) {
        Some(bytes) => SecretKey::from_bytes(&bytes),
        None => {
            let key = SecretKey::generate();
            std::fs::write(&path, bytes_to_hex(&key.to_bytes())).map_err(|e| format!("couldn't save the identity: {e}"))?;
            key
        }
    };
    Ok(key.public().to_string())
}

//! Where connection secrets live: the OS keychain (Windows Credential
//! Manager, macOS Keychain, Secret Service), one entry per key. If the
//! keychain can't be reached, a file in Vivianne's home, readable only by the
//! user — the same fallback the rest of the app's credentials use.

use serde_json::Value;

const SERVICE: &str = "vivianne";

fn entry(key: &str) -> Option<keyring::Entry> {
    keyring::Entry::new(SERVICE, &format!("conn:{key}")).ok()
}

fn file() -> Option<std::path::PathBuf> {
    crate::agent::paths::viv_home().ok().map(|h| h.join("connections.secrets.json"))
}

fn read_file() -> serde_json::Map<String, Value> {
    file().and_then(|p| std::fs::read_to_string(p).ok()).and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

fn write_file(map: &serde_json::Map<String, Value>) -> Result<(), String> {
    let p = file().ok_or("no home folder")?;
    std::fs::write(&p, serde_json::to_string(map).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}

pub async fn get(key: &str) -> Option<Value> {
    let k = key.to_string();
    tokio::task::spawn_blocking(move || {
        match entry(&k).map(|e| e.get_password()) {
            Some(Ok(s)) => serde_json::from_str(&s).ok(),
            Some(Err(keyring::Error::NoEntry)) => read_file().get(&k).cloned(),
            _ => read_file().get(&k).cloned(),
        }
    })
    .await
    .ok()
    .flatten()
}

pub async fn set(key: &str, value: &Value) -> Result<(), String> {
    let (k, json) = (key.to_string(), value.to_string());
    tokio::task::spawn_blocking(move || {
        if let Some(e) = entry(&k) {
            if e.set_password(&json).is_ok() {
                return Ok(());
            }
        }
        let mut map = read_file();
        map.insert(k, serde_json::from_str(&json).unwrap_or(Value::Null));
        write_file(&map)
    })
    .await
    .map_err(|e| e.to_string())?
}

pub async fn delete(key: &str) {
    let k = key.to_string();
    let _ = tokio::task::spawn_blocking(move || {
        if let Some(e) = entry(&k) {
            let _ = e.delete_credential();
        }
        let mut map = read_file();
        if map.remove(&k).is_some() {
            let _ = write_file(&map);
        }
    })
    .await;
}

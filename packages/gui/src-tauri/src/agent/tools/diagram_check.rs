//! `check_diagram`: lets the model validate a chemistry, biology, plot or
//! physics script before it puts one in a reply.
//!
//! The renderers live in the frontend (RDKit for chemistry, the DSL parsers
//! for the rest), and checking with anything else would answer a different
//! question from the one the chat asks when it draws the block. So the
//! request goes to the main window, which runs the very same code and calls
//! `cmd_diagram_check_done` with what it found. Same shape as the Viv Instant
//! hand-off to the main window: an id on an event, a oneshot waiting for the
//! command that answers it, and a timeout so a closed window can't hang a run.

use crate::agent::runtime::types::{AgentToolResult, MessageContent};

/// Requests waiting for the main window's verdict, by id.
static PENDING: std::sync::Mutex<Vec<(String, tokio::sync::oneshot::Sender<String>)>> = std::sync::Mutex::new(Vec::new());

/// The kinds of script the frontend can check.
const KINDS: &[&str] = &["chem", "mol3d", "bio", "plot", "physics"];

/// The main window's verdict on a script.
#[tauri::command]
pub async fn cmd_diagram_check_done(id: String, message: String) -> Result<(), String> {
    let mut pending = PENDING.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(at) = pending.iter().position(|(k, _)| *k == id) {
        let (_, tx) = pending.remove(at);
        let _ = tx.send(message);
    }
    Ok(())
}

pub fn schema() -> serde_json::Value {
    serde_json::json!({
        "name": "check_diagram",
        "description": "Check a science script before you put it in a reply. Runs the same parser and renderer the chat uses for the ```chem, ```mol3d, ```bio, ```plot and ```physics fences and tells you whether it will draw: an unclosed ring, an unknown atom, a bad Newick tree, a malformed physics line all come back with the line number. For chemistry it also returns each molecule's canonical SMILES, formula and molecular weight, so you can confirm the structure is the one you meant. Read the matching `guide` (science/chemistry, science/biology, science/physics) first; check anything non-trivial, then fix what it reports and check again.",
        "parameters": {
            "type": "object",
            "properties": {
                "kind": { "type": "string", "enum": KINDS, "description": "Which fence the script is for." },
                "code": { "type": "string", "description": "The script, exactly as it would sit inside the fence." }
            },
            "required": ["kind", "code"]
        }
    })
}

pub async fn execute(args: &serde_json::Value, app: &tauri::AppHandle) -> AgentToolResult {
    let kind = args.get("kind").and_then(|v| v.as_str()).unwrap_or("");
    let code = args.get("code").and_then(|v| v.as_str()).unwrap_or("");
    let text = if !KINDS.contains(&kind) {
        format!("Error: 'kind' must be one of {}.", KINDS.join(", "))
    } else if code.trim().is_empty() {
        "Error: 'code' is required.".to_string()
    } else {
        match ask(app, kind, code).await {
            Ok(t) => t,
            Err(e) => format!("Error: {e}"),
        }
    };
    AgentToolResult { content: vec![MessageContent::Text { text }], details: serde_json::Value::Null, terminate: None }
}

async fn ask(app: &tauri::AppHandle, kind: &str, code: &str) -> Result<String, String> {
    use tauri::Emitter;
    let id = uuid::Uuid::new_v4().simple().to_string();
    let (tx, rx) = tokio::sync::oneshot::channel();
    PENDING.lock().unwrap_or_else(|e| e.into_inner()).push((id.clone(), tx));
    let _ = app.emit_to("main", "diagram:check", serde_json::json!({ "id": id, "kind": kind, "code": code }));
    // The first chemistry check loads RDKit's WebAssembly, which takes a few
    // seconds; after that it is instant.
    match tokio::time::timeout(std::time::Duration::from_secs(45), rx).await {
        Ok(Ok(message)) => Ok(message),
        _ => {
            PENDING.lock().unwrap_or_else(|e| e.into_inner()).retain(|(k, _)| *k != id);
            Err("The Vivianne window didn't answer, so the script was not checked. Open the app and try again.".to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_names_every_kind() {
        let s = schema();
        assert_eq!(s["name"], "check_diagram");
        assert_eq!(s["parameters"]["properties"]["kind"]["enum"].as_array().map(Vec::len), Some(KINDS.len()));
    }

    #[tokio::test]
    async fn the_verdict_reaches_the_waiting_call() {
        let (tx, rx) = tokio::sync::oneshot::channel();
        PENDING.lock().unwrap().push(("t1".into(), tx));
        cmd_diagram_check_done("t1".into(), "ok".into()).await.unwrap();
        assert_eq!(rx.await.unwrap(), "ok");
    }
}

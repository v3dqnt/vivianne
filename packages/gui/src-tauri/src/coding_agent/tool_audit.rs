//! Live audit of every registry tool: runs each one for real against a scratch
//! project and reports whether it worked, how long it took, and what it said.
//!
//! Not part of the normal test run — it touches the network (web search and
//! fetch, a git clone) and takes a while. Run it with
//! `cargo test --lib tool_audit -- --ignored --nocapture`.

#![cfg(test)]

use std::time::Instant;

use serde_json::json;

use super::tool_exec::create_coding_tools;

fn scratch_project() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("viv-tool-audit-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(dir.join("src/main.py"), "def add(a, b):\n    return a + b\n\nprint(add(2, 3))\n").unwrap();
    std::fs::write(dir.join("src/util.py"), "import os\n\ndef home():\n    return os.path.expanduser('~')\n").unwrap();
    std::fs::write(dir.join("README.md"), "# Scratch\n\nA project for the tool audit. The add function sums numbers.\n").unwrap();
    std::fs::write(
        dir.join("nb.ipynb"),
        r#"{"cells":[{"cell_type":"code","id":"c1","metadata":{},"source":["x = 1"],"outputs":[],"execution_count":null}],"metadata":{},"nbformat":4,"nbformat_minor":5}"#,
    )
    .unwrap();
    dir
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn tool_audit() {
    let root = scratch_project();
    let cwd = root.to_string_lossy().to_string();
    let tools = create_coding_tools(cwd.clone());
    let shell = if cfg!(windows) { "echo hello" } else { "echo hello" };

    let cases: Vec<(&str, serde_json::Value)> = vec![
        ("read", json!({ "path": "src/main.py" })),
        ("write", json!({ "path": "src/new.py", "content": "VALUE = 42\n" })),
        ("edit", json!({ "path": "src/new.py", "old_string": "VALUE = 42", "new_string": "VALUE = 43" })),
        ("ls", json!({ "path": "." })),
        ("glob", json!({ "pattern": "**/*.py" })),
        ("grep", json!({ "pattern": "def \\w+", "path": "." })),
        ("code_search", json!({ "query": "function that sums two numbers" })),
        ("bash", json!({ "command": shell })),
        ("powershell", json!({ "command": "Write-Output hello" })),
        (
            "apply_patch",
            json!({ "patchText": "*** Begin Patch\n*** Update File: src/util.py\n@@\n-import os\n+import os\n+import sys\n*** Add File: src/added.py\n+ADDED = True\n*** End Patch" }),
        ),
        ("notebook_edit", json!({ "notebook_path": "nb.ipynb", "cell_id": "c1", "new_source": "x = 2" })),
        ("diagnostics", json!({ "path": "src/main.py" })),
        (
            "create_document",
            json!({ "format": "docx", "path": "out/report.docx", "title": "Audit", "blocks": [{ "type": "paragraph", "text": "Hello" }] }),
        ),
        (
            "create_document",
            json!({ "format": "xlsx", "path": "out/sheet.xlsx", "sheets": [{ "name": "S", "rows": [["a", "b"], [1, 2]] }] }),
        ),
        (
            "create_document",
            json!({ "format": "pptx", "path": "out/deck.pptx", "slides": [{ "title": "T", "bullets": ["one"] }] }),
        ),
        (
            "create_document",
            json!({ "format": "pdf", "path": "out/doc.pdf", "title": "Audit", "blocks": [{ "type": "paragraph", "text": "Hello" }] }),
        ),
        ("inspect_document", json!({ "path": "out/report.docx" })),
        ("edit_document", json!({ "path": "out/report.docx", "part": "p:1", "text": "Edited" })),
        ("todo_write", json!({ "todos": [{ "content": "audit tools", "status": "in_progress" }] })),
        ("sleep", json!({ "seconds": 1 })),
        ("web_search", json!({ "query": "rust programming language" })),
        ("web_search", json!({ "query": "tokio async runtime" })),
        ("web_fetch", json!({ "url": "https://example.com" })),
        ("repo_clone", json!({ "repository": "octocat/Hello-World" })),
        ("repo_overview", json!({ "repository": "octocat/Hello-World" })),
        ("enter_plan_mode", json!({ "plan": "audit" })),
        ("exit_plan_mode", json!({})),
        ("task_output", json!({ "task_ids": ["nope"] })),
        ("kill_task", json!({ "task_ids": ["nope"] })),
    ];

    println!("\n{:<18} {:<5} {:>8}  output", "tool", "ok", "ms");
    println!("{}", "-".repeat(110));
    let mut seen = std::collections::HashSet::new();
    for (name, args) in &cases {
        seen.insert(*name);
        let Some(tool) = tools.iter().find(|t| t.name() == *name) else {
            println!("{name:<18} {:<5} {:>8}  (not in the registry)", "MISS", "-");
            continue;
        };
        let started = Instant::now();
        let fut = tool.execute(args, &cwd);
        let res = tokio::time::timeout(std::time::Duration::from_secs(120), fut).await;
        let ms = started.elapsed().as_millis();
        match res {
            Ok(r) => {
                let snippet: String = r.content.replace('\n', " ⏎ ").chars().take(150).collect();
                println!("{name:<18} {:<5} {ms:>8}  {snippet}", if r.is_error { "ERR" } else { "ok" });
            }
            Err(_) => println!("{name:<18} {:<5} {ms:>8}  timed out after 120s", "HANG"),
        }
    }
    let untested: Vec<&str> = tools.iter().map(|t| t.name()).filter(|n| !seen.contains(n)).collect();
    println!("\nregistry tools not exercised here: {untested:?}");
    let _ = std::fs::remove_dir_all(&root);
}

/// What DuckDuckGo Lite hands the app's own HTTP client, across variants.
#[tokio::test]
#[ignore]
async fn ddg_probe() {
    const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";
    let variants: Vec<(&str, reqwest::Client, bool)> = vec![
        ("default", reqwest::Client::builder().user_agent(UA).build().unwrap(), false),
        ("http1", reqwest::Client::builder().user_agent(UA).http1_only().build().unwrap(), false),
        ("http1+accept", reqwest::Client::builder().user_agent(UA).http1_only().build().unwrap(), true),
        ("curl-ua+http1+accept", reqwest::Client::builder().user_agent("curl/8.9.1").http1_only().build().unwrap(), true),
    ];
    for (label, client, accept) in variants {
        let mut req = client
            .post("https://lite.duckduckgo.com/lite/")
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body("q=best+rust+web+frameworks");
        if accept {
            req = req.header("Accept", "*/*");
        }
        let r = req.send().await.unwrap();
        let status = r.status();
        let html = r.text().await.unwrap();
        println!("{label:<22} status={status} result-links={}", html.matches("result-link").count());
    }
}

/// Back-to-back searches — the burst pattern that used to get blocked — with
/// the real credential store loaded so the fallbacks can run.
#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn search_live() {
    if crate::ai::keychain_store::KeychainCredentialStore::is_available() {
        crate::ai::auth::set_global(std::sync::Arc::new(crate::ai::auth::AuthState::with_keychain_store()));
    }
    let tools = create_coding_tools(".".into());
    let search = tools.iter().find(|t| t.name() == "web_search").unwrap();
    for q in ["tokio async runtime", "serde json rust derive", "tauri v2 tray icon"] {
        let started = Instant::now();
        let r = search.execute(&json!({ "query": q }), ".").await;
        let head: String = r.content.replace('\n', " | ").chars().take(220).collect();
        println!("{:>6}ms {} {head}", started.elapsed().as_millis(), if r.is_error { "ERR" } else { "ok " });
    }
}

/// Which image and search-capable models the user's OpenAI key can use.
#[tokio::test]
#[ignore]
async fn openai_models_probe() {
    if crate::ai::keychain_store::KeychainCredentialStore::is_available() {
        crate::ai::auth::set_global(std::sync::Arc::new(crate::ai::auth::AuthState::with_keychain_store()));
    }
    let key = crate::ai::auth::stored_api_key("openai").await.expect("openai key");
    let v: serde_json::Value = reqwest::Client::new()
        .get("https://api.openai.com/v1/models")
        .bearer_auth(key)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let mut ids: Vec<String> = v["data"].as_array().unwrap().iter().filter_map(|m| m["id"].as_str().map(String::from)).collect();
    ids.sort();
    println!("image: {:?}", ids.iter().filter(|i| i.contains("image") || i.contains("dall")).collect::<Vec<_>>());
    println!("mini/nano: {:?}", ids.iter().filter(|i| (i.contains("mini") || i.contains("nano")) && !i.contains("audio") && !i.contains("realtime") && !i.contains("tts") && !i.contains("transcribe")).collect::<Vec<_>>());
}

/// Speed of candidate models for the web-search fallback, and a check that
/// the new default image model is accepted.
#[tokio::test]
#[ignore]
async fn model_upgrade_probe() {
    if crate::ai::keychain_store::KeychainCredentialStore::is_available() {
        crate::ai::auth::set_global(std::sync::Arc::new(crate::ai::auth::AuthState::with_keychain_store()));
    }
    let key = crate::ai::auth::stored_api_key("openai").await.expect("openai key");
    let c = reqwest::Client::new();
    for (model, reasoning) in [("gpt-4.1-mini", false), ("gpt-5.4-mini", true), ("gpt-5.4-nano", true)] {
        let mut body = json!({ "model": model, "tools": [{ "type": "web_search" }], "input": "Search the web for: tokio async runtime. List the top 3 results with title and URL." });
        if reasoning {
            body["reasoning"] = json!({ "effort": "low" });
        }
        let t = Instant::now();
        let r = c.post("https://api.openai.com/v1/responses").bearer_auth(&key).json(&body).send().await.unwrap();
        let status = r.status();
        let v: serde_json::Value = r.json().await.unwrap();
        let cites = v.to_string().matches("url_citation").count();
        println!("{model:<14} {status} {:>6}ms citations={cites} err={}", t.elapsed().as_millis(), v.pointer("/error/message").map(|m| m.to_string()).unwrap_or_default());
    }
    let t = Instant::now();
    let r = c
        .post("https://api.openai.com/v1/images/generations")
        .bearer_auth(&key)
        .json(&json!({ "model": "gpt-image-2", "prompt": "a small blue circle on white", "size": "1024x1024", "quality": "low", "n": 1 }))
        .send()
        .await
        .unwrap();
    let status = r.status();
    let v: serde_json::Value = r.json().await.unwrap();
    println!("gpt-image-2 {status} {}ms has_image={} err={}", t.elapsed().as_millis(), v.pointer("/data/0/b64_json").is_some(), v.pointer("/error/message").map(|m| m.to_string()).unwrap_or_default());
}

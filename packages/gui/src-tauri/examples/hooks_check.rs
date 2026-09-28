//! End-to-end check for the lifecycle-hook layer.
//!
//! `cargo test` cannot run in this crate (pre-existing STATUS_ENTRYPOINT_NOT_FOUND),
//! so the real subprocess path — config load, matcher, stdin payload, stdout
//! parse, decision — is exercised here instead. Writes a temporary
//! `~/.viv/agent/hooks.json` and restores whatever was there before.

use app_lib::agent::hooks::{run, HookEvent};

fn shell(cmd: &str) -> String {
    // Hooks run through `cmd /C` on Windows and `sh -c` elsewhere; the probes
    // below have to be written for whichever this host uses.
    let _ = cmd;
    if cfg!(windows) { "windows".into() } else { "unix".into() }
}

#[tokio::main]
async fn main() {
    let path = app_lib::agent::hooks::hooks_file_path().expect("hooks path");
    let previous = std::fs::read_to_string(&path).ok();
    println!("host shell family: {}", shell(""));

    // Three hooks: one denies bash, one adds context to every tool, and one
    // rewrites a read path. The deny is scoped by matcher so the other tools
    // must remain unaffected.
    let cfg = if cfg!(windows) {
        r#"{"hooks":[
          {"event":"PreToolUse","matcher":"bash","command":"echo {\"decision\":\"deny\",\"reason\":\"no shell today\"}"},
          {"event":"PreToolUse","matcher":"read","command":"echo {\"updatedInput\":{\"path\":\"rewritten.ts\"}}"},
          {"event":"PostToolUse","command":"echo {\"additionalContext\":\"lint clean\"}"}
        ]}"#
    } else {
        r#"{"hooks":[
          {"event":"PreToolUse","matcher":"bash","command":"echo '{\"decision\":\"deny\",\"reason\":\"no shell today\"}'"},
          {"event":"PreToolUse","matcher":"read","command":"echo '{\"updatedInput\":{\"path\":\"rewritten.ts\"}}'"},
          {"event":"PostToolUse","command":"echo '{\"additionalContext\":\"lint clean\"}'"}
        ]}"#
    };
    std::fs::write(&path, cfg).expect("write hooks.json");

    let mut failures = 0;
    let mut check = |name: &str, ok: bool, detail: String| {
        println!("{} {name}: {detail}", if ok { "PASS" } else { "FAIL" });
        if !ok {
            failures += 1;
        }
    };

    // 1. matched deny
    let o = run(HookEvent::PreToolUse, Some("bash"), serde_json::json!({"toolName":"bash"})).await;
    check("deny on matched tool", o.is_denied(), format!("{:?}", o.denied));

    // 2. the same deny must NOT catch an unrelated tool
    let o = run(HookEvent::PreToolUse, Some("glob"), serde_json::json!({"toolName":"glob"})).await;
    check("unmatched tool not denied", !o.is_denied(), format!("denied={:?}", o.denied));

    // 3. argument rewrite
    let o = run(HookEvent::PreToolUse, Some("read"), serde_json::json!({"toolName":"read"})).await;
    let rewrote = o
        .updated_input
        .as_ref()
        .and_then(|v| v.get("path"))
        .and_then(|v| v.as_str())
        == Some("rewritten.ts");
    check("argument rewrite", rewrote, format!("{:?}", o.updated_input));

    // 4. context injection on an unmatched-matcher (global) hook
    let o = run(HookEvent::PostToolUse, Some("write"), serde_json::json!({"toolName":"write"})).await;
    check(
        "post-tool context",
        o.context_block().as_deref() == Some("lint clean"),
        format!("{:?}", o.context_block()),
    );

    // 5. an event with no hooks configured must be inert
    let o = run(HookEvent::Stop, None, serde_json::json!({})).await;
    check("no hooks for event", !o.is_denied() && o.context_block().is_none(), "inert".into());

    // 6. a broken hook must fail open, not wedge the loop
    let broken = r#"{"hooks":[{"event":"PreToolUse","command":"this-command-does-not-exist-xyz"}]}"#;
    std::fs::write(&path, broken).expect("write");
    let o = run(HookEvent::PreToolUse, Some("bash"), serde_json::json!({})).await;
    check("broken hook fails open", !o.is_denied(), "not denied".into());

    // 7. malformed config must not deny everything
    std::fs::write(&path, "{ this is not json").expect("write");
    let o = run(HookEvent::PreToolUse, Some("bash"), serde_json::json!({})).await;
    check("malformed config fails open", !o.is_denied(), "not denied".into());

    match previous {
        Some(p) => std::fs::write(&path, p).expect("restore"),
        None => {
            let _ = std::fs::remove_file(&path);
        }
    }
    println!("\n{}", if failures == 0 { "ALL CHECKS PASSED" } else { "SOME CHECKS FAILED" });
    std::process::exit(if failures == 0 { 0 } else { 1 });
}

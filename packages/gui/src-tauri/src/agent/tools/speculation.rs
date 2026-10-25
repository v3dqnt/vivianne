//! Speculative tool calling: start read-only tool calls while the model is
//! still writing, so their latency hides behind token generation.
//!
//! Two sources, one store:
//!
//! - **Direct calls.** A provider streams each tool call complete before the
//!   reply ends (often several in one reply). Each eligible one starts the
//!   moment its arguments close, not after the whole message.
//! - **Calls inside a `run_script` program** (speculative PTC). While the
//!   model streams the script's `code`, complete calls with all-literal
//!   arguments — `read("src/a.rs")`, `grep("TODO", "src")`,
//!   `callTool("code_search", {query: "x"})` — start before the script exists.
//!
//! The contract: a served result is indistinguishable from the call having
//! run at its real point. So a result is served only when
//! - the same tool with equal arguments in the same folder asks for it,
//! - nothing that changes things has run since it started (`EPOCH`),
//! - for `read`, the file's size and modified time are unchanged,
//! - it is younger than `TTL`;
//!
//! and only once. Anything else runs the call normally, and a speculation
//! that failed or was never asked for is simply dropped. Only calls the fast
//! path would run without asking are eligible (`exec::fast_path_ok`): pure
//! local reads inside the workspace, nothing gated, nothing disabled.

use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use futures::future::{BoxFuture, FutureExt, Shared};

use crate::coding_agent::tool_exec::ToolResult;

/// How long an unclaimed speculation stays servable. Short on purpose: it
/// bounds how stale a grep or listing can be against an edit made outside
/// the agent, which the epoch can't see.
const TTL: Duration = Duration::from_secs(30);
/// Unclaimed speculations kept at once, oldest dropped first.
const MAX_ENTRIES: usize = 32;
/// The biggest script prefix scanned for calls.
const MAX_SCRIPT_BYTES: usize = 256 * 1024;

/// Bumped by anything that may change what a read sees.
static EPOCH: AtomicU64 = AtomicU64::new(0);

struct Entry {
    cwd: String,
    name: String,
    args: serde_json::Value,
    epoch: u64,
    started: Instant,
    /// `read`'s file as it was at launch: (modified, size).
    stamp: Option<(SystemTime, u64)>,
    result: Shared<BoxFuture<'static, Option<ToolResult>>>,
}

static STORE: Mutex<Vec<Entry>> = Mutex::new(Vec::new());

fn store() -> std::sync::MutexGuard<'static, Vec<Entry>> {
    STORE.lock().unwrap_or_else(|e| e.into_inner())
}

/// The parts of an `ai-stream-token` event speculation reads: a tool call
/// the model just finished writing (sent as soon as its arguments close, not
/// with the whole message), or a piece of a `run_script` call's argument JSON
/// as it streams. Absent from every other event.
#[derive(Debug, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Streamed {
    #[serde(default)]
    pub tool_call: Option<StreamedToolCall>,
    #[serde(default)]
    pub tool_input: Option<StreamedToolInput>,
}

#[derive(Debug, serde::Deserialize)]
pub struct StreamedToolCall {
    pub name: String,
    #[serde(default)]
    pub arguments: serde_json::Value,
}

#[derive(Debug, serde::Deserialize)]
pub struct StreamedToolInput {
    pub id: String,
    pub delta: String,
}

/// What decides eligibility, captured once per model call.
#[derive(Clone)]
pub struct Scope {
    pub cwd: String,
    pub workspace_folders: Vec<String>,
    pub disabled: Vec<String>,
}

impl Scope {
    pub async fn of(state: &crate::agent::runtime::state::AgentState) -> Self {
        let workspace_folders = state.workspace_folders().await;
        Scope {
            cwd: workspace_folders.first().cloned().unwrap_or_else(|| ".".to_string()),
            disabled: super::exec::compute_restrictions(state).await.disabled_tools,
            workspace_folders,
        }
    }
}

tokio::task_local! {
    /// Set around a model call by the agent loop; the stream reads it.
    pub static SCOPE: Arc<Scope>;
}

/// A tool that may change what a read would see is about to run, or just
/// ran: everything speculated before now is stale. `run_script` and `task`
/// only orchestrate — their own calls report themselves — so they don't
/// count, or a script's speculations would die the moment it started.
pub fn note_call(name: &str) {
    use super::traits::Effect;
    if name == "run_script" {
        return;
    }
    if !matches!(super::traits::effect(name), Some(Effect::Read | Effect::Talk | Effect::Delegate)) {
        EPOCH.fetch_add(1, Ordering::SeqCst);
    }
}

fn stamp(cwd: &str, name: &str, args: &serde_json::Value) -> Option<(SystemTime, u64)> {
    if name != "read" {
        return None;
    }
    let path = crate::coding_agent::tool_exec::resolve_path(cwd, args.get("path")?.as_str()?);
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.modified().ok()?, meta.len()))
}

/// Start `name(args)` now if it's eligible and not already running.
pub fn launch(scope: &Scope, name: &str, args: serde_json::Value) {
    if !super::exec::fast_path_ok(name, &args, &scope.disabled, &scope.workspace_folders, &scope.cwd) {
        return;
    }
    let mut entries = store();
    entries.retain(|e| e.started.elapsed() < TTL);
    if entries.iter().any(|e| e.name == name && e.cwd == scope.cwd && e.args == args) {
        return;
    }
    let (cwd, tool, call_args) = (scope.cwd.clone(), name.to_string(), args.clone());
    let task = tokio::spawn(async move {
        let tools = crate::coding_agent::tool_exec::create_coding_tools(cwd);
        match tools.iter().find(|t| t.name() == tool) {
            Some(t) => Some(t.execute(&call_args, ".").await),
            None => None,
        }
    });
    let result = async move { task.await.ok().flatten() }.boxed().shared();
    if entries.len() >= MAX_ENTRIES {
        entries.remove(0);
    }
    entries.push(Entry {
        stamp: stamp(&scope.cwd, name, &args),
        cwd: scope.cwd.clone(),
        name: name.to_string(),
        args,
        epoch: EPOCH.load(Ordering::SeqCst),
        started: Instant::now(),
        result,
    });
}

/// The speculated result for this call, if there is one that's still true.
/// Take-once: a second identical call runs for real.
pub async fn take(cwd: &str, name: &str, args: &serde_json::Value) -> Option<ToolResult> {
    let entry = {
        let mut entries = store();
        let at = entries.iter().position(|e| e.name == name && e.cwd == cwd && &e.args == args)?;
        entries.remove(at)
    };
    let result = entry.result.await?;
    // Checked after the await: a mutation may have landed while it ran.
    let fresh = entry.epoch == EPOCH.load(Ordering::SeqCst)
        && entry.started.elapsed() < TTL
        && entry.stamp == stamp(cwd, name, args);
    fresh.then_some(result)
}

// ── Speculative PTC: calls inside a streaming `run_script` program ─────────

/// One streaming `run_script` call's arguments, scanned as they arrive.
#[derive(Default)]
pub struct ScriptScan {
    raw: String,
    launched: HashSet<String>,
}

impl ScriptScan {
    /// Feed the next chunk of the call's argument JSON; start whatever new
    /// complete literal calls it now contains.
    pub fn feed(&mut self, scope: &Scope, chunk: &str) {
        if self.raw.len() + chunk.len() > MAX_SCRIPT_BYTES {
            return;
        }
        self.raw.push_str(chunk);
        // A call can only have just completed if its `)` just arrived.
        if !chunk.contains(')') {
            return;
        }
        let Some(code) = partial_code(&self.raw) else { return };
        for (name, args) in literal_calls(&code) {
            let key = format!("{name}{args}");
            if self.launched.insert(key) {
                launch(scope, &name, args);
            }
        }
    }
}

/// The `code` string of a partial `{"code": "...` argument object, unescaped
/// as far as it has arrived.
fn partial_code(raw: &str) -> Option<String> {
    let start = raw.find("\"code\"")? + "\"code\"".len();
    let rest = raw[start..].trim_start().strip_prefix(':')?.trim_start().strip_prefix('"')?;
    let mut out = String::new();
    let mut chars = rest.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => break,
            '\\' => match chars.next()? {
                'n' => out.push('\n'),
                't' => out.push('\t'),
                'r' => out.push('\r'),
                'b' => out.push('\u{8}'),
                'f' => out.push('\u{c}'),
                'u' => {
                    let hex: String = chars.by_ref().take(4).collect();
                    out.push(char::from_u32(u32::from_str_radix(&hex, 16).ok()?).unwrap_or('\u{fffd}'));
                }
                other => out.push(other),
            },
            c => out.push(c),
        }
    }
    Some(out)
}

/// The script shorthands `ptc::SHIM` defines with positional arguments, and
/// how they map onto the tool's own arguments.
fn positional(name: &str, args: Vec<serde_json::Value>) -> Option<serde_json::Value> {
    use serde_json::{Map, Value};
    let mut it = args.into_iter();
    let mut obj = Map::new();
    let mut put = |key: &str, v: Option<Value>| {
        if let Some(v) = v.filter(|v| !v.is_null()) {
            obj.insert(key.to_string(), v);
        }
    };
    match name {
        "read" | "ls" => {
            put("path", it.next());
            if let Some(opts) = it.next() {
                obj.extend(opts.as_object()?.clone());
            }
        }
        "grep" => {
            put("pattern", it.next());
            put("path", it.next());
        }
        "glob" => {
            put("pattern", it.next());
            put("path", it.next());
        }
        _ => return it.next().filter(|v| v.is_object()),
    }
    Some(Value::Object(obj))
}

/// Every complete call in `code` whose arguments are all literals, as
/// (tool, arguments) — in the shapes a script can write them: the
/// shorthands, a bare tool global with one object, `tools.<name>({...})` and
/// `callTool("<name>", {...})`. Names the script rebinds itself are skipped:
/// their calls aren't the tool's.
fn literal_calls(code: &str) -> Vec<(String, serde_json::Value)> {
    let bytes = code.as_bytes();
    let ident = |b: u8| b.is_ascii_alphanumeric() || b == b'_' || b == b'$';
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if !ident(bytes[i]) || (i > 0 && ident(bytes[i - 1])) {
            i += 1;
            continue;
        }
        let start = i;
        while i < bytes.len() && ident(bytes[i]) {
            i += 1;
        }
        let word = &code[start..i];
        let after_dot = start > 0 && bytes[start - 1] == b'.';
        let via_tools = after_dot && code[..start - 1].ends_with("tools");
        if after_dot && !via_tools {
            continue;
        }
        let mut j = i;
        while j < bytes.len() && bytes[j].is_ascii_whitespace() {
            j += 1;
        }
        if j >= bytes.len() || bytes[j] != b'(' {
            continue;
        }
        let Some((args, end)) = parse_args(code, j + 1) else { continue };
        let call = if word == "callTool" && !after_dot {
            match args.as_slice() {
                [serde_json::Value::String(name), rest @ ..] => {
                    let a = rest.first().cloned().unwrap_or_else(|| serde_json::json!({}));
                    a.is_object().then(|| (name.clone(), a))
                }
                _ => None,
            }
        } else if via_tools {
            args.into_iter().next().filter(|a| a.is_object()).map(|a| (word.to_string(), a))
        } else {
            positional(word, args).map(|a| (word.to_string(), a))
        };
        if let Some((name, a)) = call {
            if !rebinds(code, &name) {
                out.push((name, a));
            }
        }
        i = end;
    }
    out
}

/// Whether the script declares or assigns `name` itself.
fn rebinds(code: &str, name: &str) -> bool {
    ["const ", "let ", "var ", "function ", "class "].iter().any(|kw| code.contains(&format!("{kw}{name}")))
        || code.contains(&format!("{name} =")) && !code.contains(&format!("{name} =="))
}

/// Parse a call's arguments from just after `(` up to its `)`, all
/// literals. `None` for anything else, or a call that hasn't finished
/// streaming. Returns the arguments and the index after `)`.
fn parse_args(code: &str, from: usize) -> Option<(Vec<serde_json::Value>, usize)> {
    let mut p = Lit { s: code.as_bytes(), i: from };
    let mut args = Vec::new();
    loop {
        p.ws();
        if p.peek()? == b')' {
            return Some((args, p.i + 1));
        }
        args.push(p.value()?);
        p.ws();
        match p.peek()? {
            b',' => p.i += 1,
            b')' => return Some((args, p.i + 1)),
            _ => return None,
        }
    }
}

/// A tiny JavaScript-literal reader: strings, numbers, booleans, null,
/// undefined, arrays and objects. `undefined` becomes `null`, which callers
/// drop, as `JSON.stringify` does.
struct Lit<'a> {
    s: &'a [u8],
    i: usize,
}

impl Lit<'_> {
    fn peek(&self) -> Option<u8> {
        self.s.get(self.i).copied()
    }
    fn ws(&mut self) {
        while self.peek().is_some_and(|b| b.is_ascii_whitespace()) {
            self.i += 1;
        }
    }
    fn value(&mut self) -> Option<serde_json::Value> {
        use serde_json::Value;
        self.ws();
        match self.peek()? {
            b'"' | b'\'' => self.string().map(Value::String),
            b'[' => {
                self.i += 1;
                let mut items = Vec::new();
                loop {
                    self.ws();
                    if self.peek()? == b']' {
                        self.i += 1;
                        return Some(Value::Array(items));
                    }
                    items.push(self.value()?);
                    self.ws();
                    match self.peek()? {
                        b',' => self.i += 1,
                        b']' => {}
                        _ => return None,
                    }
                }
            }
            b'{' => {
                self.i += 1;
                let mut map = serde_json::Map::new();
                loop {
                    self.ws();
                    match self.peek()? {
                        b'}' => {
                            self.i += 1;
                            return Some(Value::Object(map));
                        }
                        b'"' | b'\'' => {
                            let key = self.string()?;
                            self.entry(&mut map, key)?;
                        }
                        b if b.is_ascii_alphabetic() || b == b'_' || b == b'$' => {
                            let key = self.word();
                            self.entry(&mut map, key)?;
                        }
                        _ => return None,
                    }
                    self.ws();
                    match self.peek()? {
                        b',' => self.i += 1,
                        b'}' => {}
                        _ => return None,
                    }
                }
            }
            b'-' | b'0'..=b'9' => {
                let start = self.i;
                self.i += 1;
                while self.peek().is_some_and(|b| b.is_ascii_digit() || matches!(b, b'.' | b'e' | b'E' | b'+' | b'-')) {
                    self.i += 1;
                }
                let n: f64 = std::str::from_utf8(&self.s[start..self.i]).ok()?.parse().ok()?;
                // JSON.stringify writes 10.0 as 10.
                Some(if n.fract() == 0.0 && n.abs() < 9e15 { Value::from(n as i64) } else { Value::from(n) })
            }
            _ => match self.word().as_str() {
                "true" => Some(Value::Bool(true)),
                "false" => Some(Value::Bool(false)),
                "null" | "undefined" => Some(Value::Null),
                _ => None,
            },
        }
    }
    fn entry(&mut self, map: &mut serde_json::Map<String, serde_json::Value>, key: String) -> Option<()> {
        self.ws();
        if self.peek()? != b':' {
            return None;
        }
        self.i += 1;
        let v = self.value()?;
        if !v.is_null() {
            map.insert(key, v);
        }
        Some(())
    }
    fn word(&mut self) -> String {
        let start = self.i;
        while self.peek().is_some_and(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'$') {
            self.i += 1;
        }
        String::from_utf8_lossy(&self.s[start..self.i]).into_owned()
    }
    fn string(&mut self) -> Option<String> {
        let quote = self.peek()?;
        self.i += 1;
        let mut out = Vec::new();
        loop {
            let b = self.peek()?;
            self.i += 1;
            match b {
                b'\\' => {
                    let e = self.peek()?;
                    self.i += 1;
                    out.push(match e {
                        b'n' => b'\n',
                        b't' => b'\t',
                        b'r' => b'\r',
                        other => other,
                    });
                }
                b if b == quote => return String::from_utf8(out).ok(),
                b'\n' => return None,
                b => out.push(b),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn finds_literal_calls_in_every_script_shape() {
        let code = r#"
const a = await read("src/main.rs");
const b = await read('src/lib.rs', {offset: 10, limit: 20});
const hits = await grep("TODO", "src");
await callTool("code_search", { query: "speculation" });
await tools.glob({ pattern: "**/*.rs" });
await read(path);
await bash("rm -rf x");
await read("unfinished
"#;
        let calls = literal_calls(code);
        assert!(calls.contains(&("read".into(), json!({"path": "src/main.rs"}))));
        assert!(calls.contains(&("read".into(), json!({"path": "src/lib.rs", "offset": 10, "limit": 20}))));
        assert!(calls.contains(&("grep".into(), json!({"pattern": "TODO", "path": "src"}))));
        assert!(calls.contains(&("code_search".into(), json!({"query": "speculation"}))));
        assert!(calls.contains(&("glob".into(), json!({"pattern": "**/*.rs"}))));
        assert!(!calls.iter().any(|(_, a)| a.get("path") == Some(&json!("unfinished"))), "an unfinished call isn't one yet");
        assert_eq!(calls.iter().filter(|(n, _)| n == "read").count(), 2, "read(path) has a non-literal argument");
        assert!(!calls.iter().any(|(n, _)| n == "bash"), "a shorthand with a string argument and no mapping is not a call");
    }

    #[test]
    fn a_rebound_name_is_not_the_tool() {
        assert!(literal_calls("const read = (p) => p; read(\"x\")").is_empty());
    }

    #[test]
    fn partial_code_unescapes_what_has_arrived() {
        assert_eq!(partial_code(r#"{"code": "await read(\"a.rs\")\nawait gr"#).unwrap(), "await read(\"a.rs\")\nawait gr");
        assert!(partial_code(r#"{"timeout_seconds": 5"#).is_none());
    }

    #[test]
    fn only_effectful_tools_make_speculation_stale() {
        let before = EPOCH.load(Ordering::SeqCst);
        note_call("read");
        note_call("run_script");
        note_call("task");
        assert_eq!(EPOCH.load(Ordering::SeqCst), before);
        note_call("edit");
        assert!(EPOCH.load(Ordering::SeqCst) > before);
        note_call("mcp__server__anything");
        assert!(EPOCH.load(Ordering::SeqCst) > before + 1, "an unknown tool counts as a change");
    }

    /// The whole contract, against a real file: served when nothing changed,
    /// re-run when the file did, and only once.
    #[tokio::test]
    async fn serves_only_what_is_still_true() {
        let dir = std::env::temp_dir().join(format!("viv-spec-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.txt"), "one").unwrap();
        let root = dir.to_string_lossy().to_string();
        let scope = Scope { cwd: root.clone(), workspace_folders: vec![root.clone()], disabled: vec![] };
        let args = json!({ "path": "a.txt" });

        launch(&scope, "read", args.clone());
        let served = take(&root, "read", &args).await.expect("served");
        assert!(served.content.contains("one"), "{}", served.content);
        assert!(take(&root, "read", &args).await.is_none(), "take-once");

        launch(&scope, "read", args.clone());
        tokio::time::sleep(Duration::from_millis(50)).await;
        std::fs::write(dir.join("a.txt"), "two, longer").unwrap();
        assert!(take(&root, "read", &args).await.is_none(), "the file changed: run it for real");

        launch(&scope, "bash", json!({ "command": "echo hi" }));
        assert!(take(&root, "bash", &json!({ "command": "echo hi" })).await.is_none(), "never speculates a command");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

//! Native scripts: the model writes a Rust or C++ program that calls its tools.
//!
//! The same idea as `run_script` (one turn instead of a round-trip per call;
//! only what the program prints comes back), for jobs that want a compiled
//! language: heavy parsing, big refactors over thousands of files, anything
//! where a script's speed or a library's types matter.
//!
//! The program is compiled with the system's `rustc` or `g++` (cached by a
//! hash of its source, so a rerun costs nothing), started in the workspace,
//! and talks to Vivianne over its own stdin and stdout. A tool call is a line
//! on stdout, `\x01VIV <id> <tool> <byte-length>`, followed by that many bytes
//! of JSON arguments; the answer comes back on stdin as `OK <length>` or
//! `ERR <length>` and that many bytes of text. Lengths, not escaping, so
//! neither side parses JSON to read a reply. Every other line the program
//! prints is its output.
//!
//! Each call goes through `execute_tool_calls` exactly like a call from
//! `run_script` (see `ptc::call_bridged_tool`): approval, restrictions and the
//! events the chat draws all apply, and a script cannot start another script.

use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::AtomicU32;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

use crate::agent::runtime::events::EventSender;
use crate::agent::runtime::state::AgentState;
use crate::agent::runtime::types::{AgentToolResult, MessageContent};

const DEFAULT_TIMEOUT_SECS: u64 = 60;
const MAX_TIMEOUT_SECS: u64 = 600;
const COMPILE_TIMEOUT_SECS: u64 = 180;
/// Part of the cache key: a new SDK means a recompile.
const SDK_VERSION: &str = "1";

/// What the program includes first: the calls into Vivianne.
const SDK_RS: &str = r##"// Vivianne's side of the bridge. `use viv::*;` is already in scope.
#![allow(dead_code)]
pub mod viv {
    use std::io::{BufRead, Read, Write};
    use std::sync::Mutex;

    static LOCK: Mutex<u64> = Mutex::new(0);

    /// Call any tool by name with its JSON arguments; the tool's text output, or its error.
    pub fn call(tool: &str, args_json: &str) -> Result<String, String> {
        let mut id = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        *id += 1;
        {
            let out = std::io::stdout();
            let mut out = out.lock();
            write!(out, "\x01VIV {} {} {}\n", *id, tool, args_json.len()).map_err(|e| e.to_string())?;
            out.write_all(args_json.as_bytes()).map_err(|e| e.to_string())?;
            out.write_all(b"\n").map_err(|e| e.to_string())?;
            out.flush().map_err(|e| e.to_string())?;
        }
        let stdin = std::io::stdin();
        let mut stdin = stdin.lock();
        let mut header = String::new();
        stdin.read_line(&mut header).map_err(|e| e.to_string())?;
        let mut parts = header.trim().split(' ');
        let ok = parts.next() == Some("OK");
        let len: usize = parts.next().and_then(|n| n.parse().ok()).ok_or("the host sent a broken reply")?;
        let mut body = vec![0u8; len];
        stdin.read_exact(&mut body).map_err(|e| e.to_string())?;
        let text = String::from_utf8_lossy(&body).into_owned();
        if ok { Ok(text) } else { Err(text) }
    }

    /// `s` as a JSON string literal, quotes included.
    pub fn json(s: &str) -> String {
        let mut o = String::with_capacity(s.len() + 2);
        o.push('"');
        for c in s.chars() {
            match c {
                '"' => o.push_str("\\\""),
                '\\' => o.push_str("\\\\"),
                '\n' => o.push_str("\\n"),
                '\r' => o.push_str("\\r"),
                '\t' => o.push_str("\\t"),
                c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
                c => o.push(c),
            }
        }
        o.push('"');
        o
    }

    pub fn read(path: &str) -> Result<String, String> { call("read", &format!("{{\"path\":{}}}", json(path))) }
    pub fn write(path: &str, content: &str) -> Result<String, String> { call("write", &format!("{{\"path\":{},\"content\":{}}}", json(path), json(content))) }
    pub fn edit(path: &str, old: &str, new: &str) -> Result<String, String> { call("edit", &format!("{{\"path\":{},\"old_string\":{},\"new_string\":{}}}", json(path), json(old), json(new))) }
    pub fn grep(pattern: &str, path: &str) -> Result<String, String> { call("grep", &format!("{{\"pattern\":{},\"path\":{}}}", json(pattern), json(path))) }
    pub fn glob(pattern: &str, path: &str) -> Result<String, String> { call("glob", &format!("{{\"pattern\":{},\"path\":{}}}", json(pattern), json(path))) }
    pub fn bash(command: &str) -> Result<String, String> { call("bash", &format!("{{\"command\":{}}}", json(command))) }
}
use viv::*;
"##;

const SDK_CPP: &str = r##"// Vivianne's side of the bridge, already included.
#pragma once
#include <cstdio>
#include <iostream>
#include <stdexcept>
#include <string>
#include <mutex>
#ifdef _WIN32
#include <fcntl.h>
#include <io.h>
#endif

namespace viv {
inline std::mutex& lock_() { static std::mutex m; return m; }
inline void binary_mode_() {
#ifdef _WIN32
    static bool done = (_setmode(_fileno(stdin), _O_BINARY), _setmode(_fileno(stdout), _O_BINARY), true);
    (void)done;
#endif
}

/// `s` as a JSON string literal, quotes included.
inline std::string json(const std::string& s) {
    std::string o = "\"";
    for (unsigned char c : s) {
        switch (c) {
            case '"': o += "\\\""; break;
            case '\\': o += "\\\\"; break;
            case '\n': o += "\\n"; break;
            case '\r': o += "\\r"; break;
            case '\t': o += "\\t"; break;
            default:
                if (c < 0x20) { char b[8]; std::snprintf(b, sizeof b, "\\u%04x", c); o += b; }
                else o += (char)c;
        }
    }
    return o + "\"";
}

/// Call any tool by name with its JSON arguments; the tool's text output. Throws std::runtime_error with the tool's error.
inline std::string call(const std::string& tool, const std::string& args_json) {
    std::lock_guard<std::mutex> g(lock_());
    binary_mode_();
    static unsigned long id = 0;
    ++id;
    std::printf("\x01VIV %lu %s %zu\n", id, tool.c_str(), args_json.size());
    std::fwrite(args_json.data(), 1, args_json.size(), stdout);
    std::fputc('\n', stdout);
    std::fflush(stdout);
    char tag[8]; size_t len = 0;
    if (std::scanf("%7s %zu", tag, &len) != 2) throw std::runtime_error("the host sent a broken reply");
    std::fgetc(stdin);  // the newline after the header
    std::string body(len, '\0');
    if (len && std::fread(&body[0], 1, len, stdin) != len) throw std::runtime_error("the host's reply was cut short");
    if (std::string(tag) != "OK") throw std::runtime_error(body);
    return body;
}

inline std::string read(const std::string& path) { return call("read", "{\"path\":" + json(path) + "}"); }
inline std::string write(const std::string& path, const std::string& content) { return call("write", "{\"path\":" + json(path) + ",\"content\":" + json(content) + "}"); }
inline std::string edit(const std::string& path, const std::string& o, const std::string& n) { return call("edit", "{\"path\":" + json(path) + ",\"old_string\":" + json(o) + ",\"new_string\":" + json(n) + "}"); }
inline std::string grep(const std::string& pattern, const std::string& path) { return call("grep", "{\"pattern\":" + json(pattern) + ",\"path\":" + json(path) + "}"); }
inline std::string glob(const std::string& pattern, const std::string& path) { return call("glob", "{\"pattern\":" + json(pattern) + ",\"path\":" + json(path) + "}"); }
inline std::string bash(const std::string& command) { return call("bash", "{\"command\":" + json(command) + "}"); }
}  // namespace viv
"##;

#[derive(Clone, Copy, PartialEq, Debug)]
enum Lang {
    Rust,
    Cpp,
}

impl Lang {
    fn parse(s: &str) -> Option<Lang> {
        match s.to_lowercase().as_str() {
            "rust" | "rs" => Some(Lang::Rust),
            "cpp" | "c++" | "cxx" => Some(Lang::Cpp),
            _ => None,
        }
    }
    fn dir_name(self) -> &'static str {
        match self {
            Lang::Rust => "rust",
            Lang::Cpp => "cpp",
        }
    }
    fn compiler(self) -> &'static str {
        match self {
            Lang::Rust => "rustc",
            Lang::Cpp => "g++",
        }
    }
    fn install_hint(self) -> &'static str {
        match self {
            Lang::Rust => "Install Rust from rustup.rs.",
            Lang::Cpp => "Install a C++ compiler: g++ (MinGW-w64 on Windows) or clang++, and put it on PATH.",
        }
    }
}

/// The source the compiler is given: the SDK, then the model's code.
fn assemble(lang: Lang, code: &str) -> String {
    match lang {
        Lang::Rust => format!("{SDK_RS}\n{code}\n"),
        Lang::Cpp => format!("#include \"viv.hpp\"\n{code}\n"),
    }
}

fn cache_key(lang: Lang, code: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (SDK_VERSION, lang.dir_name(), code).hash(&mut h);
    format!("{:016x}", h.finish())
}

fn exe_name() -> &'static str {
    if cfg!(windows) { "prog.exe" } else { "prog" }
}

fn work_dir(lang: Lang, key: &str) -> PathBuf {
    crate::agent::system::paths::viv_home_path(&["native", &format!("{}-{key}", lang.dir_name())])
}

/// Compile unless this exact program was compiled before. The compiler's
/// complaints come back as the error, trimmed to the part worth reading.
async fn compile(lang: Lang, code: &str) -> Result<(PathBuf, bool), String> {
    let compiler = which::which(lang.compiler()).map_err(|_| format!("`{}` isn't installed or isn't on PATH. {}", lang.compiler(), lang.install_hint()))?;
    let dir = work_dir(lang, &cache_key(lang, code));
    let exe = dir.join(exe_name());
    if exe.exists() {
        return Ok((exe, true));
    }
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let (src, sdk) = match lang {
        Lang::Rust => ("main.rs", None),
        Lang::Cpp => ("main.cpp", Some(("viv.hpp", SDK_CPP))),
    };
    std::fs::write(dir.join(src), assemble(lang, code)).map_err(|e| e.to_string())?;
    if let Some((name, text)) = sdk {
        std::fs::write(dir.join(name), text).map_err(|e| e.to_string())?;
    }
    let mut command = tokio::process::Command::new(compiler);
    match lang {
        Lang::Rust => command.args(["--edition", "2021", "-C", "opt-level=2", "main.rs", "-o", exe_name()]),
        Lang::Cpp => command.args(["-std=c++17", "-O2", "main.cpp", "-o", exe_name()]),
    };
    command.current_dir(&dir).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    let child = command.spawn().map_err(|e| format!("Couldn't start {}: {e}", lang.compiler()))?;
    let out = tokio::time::timeout(Duration::from_secs(COMPILE_TIMEOUT_SECS), child.wait_with_output())
        .await
        .map_err(|_| format!("Compiling took over {COMPILE_TIMEOUT_SECS}s."))?
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        let _ = std::fs::remove_dir_all(&dir);
        let err = String::from_utf8_lossy(&out.stderr);
        let lines: Vec<&str> = err.lines().collect();
        let shown = if lines.len() > 40 { &lines[..40] } else { &lines[..] };
        return Err(format!("It didn't compile:\n{}", shown.join("\n")));
    }
    Ok((exe, false))
}

pub(crate) async fn execute_native_tool(args: &serde_json::Value, delegate_depth: usize, state: &AgentState, emit: &EventSender, app: &tauri::AppHandle, outer_call_id: &str) -> AgentToolResult {
    let text = |t: String| AgentToolResult { content: vec![MessageContent::Text { text: crate::agent::tools::exec::clip_tool_text(&t, crate::agent::tools::exec::MAX_UNBOUNDED_TOOL_CHARS) }], details: serde_json::Value::Null, terminate: None };
    let Some(lang) = args.get("language").and_then(|l| l.as_str()).and_then(Lang::parse) else {
        return text("Error: 'language' must be \"rust\" or \"cpp\".".into());
    };
    let code = match args.get("code").and_then(|v| v.as_str()) {
        Some(c) if !c.trim().is_empty() => c.to_string(),
        _ => return text("Error: 'code' is required.".into()),
    };
    let timeout = args.get("timeout_seconds").and_then(|v| v.as_u64()).unwrap_or(DEFAULT_TIMEOUT_SECS).clamp(1, MAX_TIMEOUT_SECS);

    let (exe, cached) = match compile(lang, &code).await {
        Ok(v) => v,
        Err(e) => return text(format!("Error: {e}")),
    };
    let cwd = state.primary_workspace_folder().await.map(PathBuf::from).or_else(|| std::env::current_dir().ok());
    let mut command = tokio::process::Command::new(&exe);
    command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
    if let Some(c) = &cwd {
        command.current_dir(c);
    }
    #[cfg(windows)]
    command.creation_flags(0x0800_0000);
    let mut child = match command.spawn() {
        Ok(c) => c,
        Err(e) => return text(format!("Error: couldn't start the program: {e}")),
    };
    let (Some(mut stdin), Some(stdout), Some(stderr)) = (child.stdin.take(), child.stdout.take(), child.stderr.take()) else {
        return text("Error: the program's pipes didn't open.".into());
    };
    let err_task = tokio::spawn(async move {
        let mut s = String::new();
        let _ = BufReader::new(stderr).read_to_string(&mut s).await;
        s
    });

    let seq = AtomicU32::new(0);
    let session = async {
        let mut out = BufReader::new(stdout);
        let mut printed = String::new();
        let mut line = Vec::new();
        loop {
            line.clear();
            if out.read_until(b'\n', &mut line).await? == 0 {
                break;
            }
            if line.first() != Some(&1) {
                printed.push_str(&String::from_utf8_lossy(&line));
                continue;
            }
            // \x01VIV <id> <tool> <length>
            let header = String::from_utf8_lossy(&line[1..]).trim().to_string();
            let mut p = header.split(' ');
            let (_, _id, tool, len) = (p.next(), p.next(), p.next().unwrap_or("").to_string(), p.next().and_then(|n| n.parse::<usize>().ok()));
            let Some(len) = len else {
                printed.push_str("[bridge: a call had no length]\n");
                continue;
            };
            let mut body = vec![0u8; len + 1];
            out.read_exact(&mut body).await?;
            body.truncate(len);
            let arguments: serde_json::Value = serde_json::from_slice(&body).unwrap_or(serde_json::Value::Null);
            let reply = crate::agent::tools::ptc::call_bridged_tool(&tool, arguments, &seq, outer_call_id, delegate_depth, state, emit, app).await;
            let (ok, text) = match serde_json::from_str::<serde_json::Value>(&reply) {
                Ok(v) if v["ok"] == true => (true, v["text"].as_str().unwrap_or("").to_string()),
                Ok(v) => (false, v["error"].as_str().unwrap_or("the call failed").to_string()),
                Err(_) => (false, reply),
            };
            let head = format!("{} {}\n", if ok { "OK" } else { "ERR" }, text.len());
            stdin.write_all(head.as_bytes()).await?;
            stdin.write_all(text.as_bytes()).await?;
            stdin.flush().await?;
        }
        Ok::<String, std::io::Error>(printed)
    };

    let finished = tokio::time::timeout(Duration::from_secs(timeout), session).await;
    let printed = match finished {
        Ok(Ok(p)) => p,
        Ok(Err(e)) => format!("[bridge error: {e}]\n"),
        Err(_) => {
            let _ = child.kill().await;
            return text(format!("Error: the program ran over {timeout}s and was stopped."));
        }
    };
    let status = child.wait().await.ok();
    let stderr_text = err_task.await.unwrap_or_default();
    let mut result = printed;
    if let Some(s) = status.filter(|s| !s.success()) {
        let tail: Vec<&str> = stderr_text.lines().rev().take(20).collect::<Vec<_>>().into_iter().rev().collect();
        result.push_str(&format!("\n[exited with {s}]\n{}", tail.join("\n")));
        return text(format!("Error: {}", result.trim()));
    }
    if result.trim().is_empty() {
        result = "(the program printed nothing)".into();
    }
    if !stderr_text.trim().is_empty() {
        result.push_str(&format!("\n[stderr]\n{}", stderr_text.trim()));
    }
    let note = if cached { "" } else { "" };
    text(format!("{}{note}", result.trim_end()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn languages_are_read_by_their_usual_names() {
        assert_eq!(Lang::parse("Rust"), Some(Lang::Rust));
        assert_eq!(Lang::parse("c++"), Some(Lang::Cpp));
        assert_eq!(Lang::parse("python"), None);
    }

    #[test]
    fn the_same_program_is_compiled_once_and_a_changed_one_again() {
        let a = cache_key(Lang::Rust, "fn main() {}");
        assert_eq!(a, cache_key(Lang::Rust, "fn main() {}"));
        assert_ne!(a, cache_key(Lang::Rust, "fn main() { }"));
        assert_ne!(cache_key(Lang::Rust, "x"), cache_key(Lang::Cpp, "x"));
    }

    #[test]
    fn the_sdk_comes_before_the_models_code() {
        assert!(assemble(Lang::Rust, "fn main() {}").contains("pub mod viv"));
        assert!(assemble(Lang::Rust, "fn main() {}").ends_with("fn main() {}\n"));
        assert!(assemble(Lang::Cpp, "int main() {}").starts_with("#include \"viv.hpp\""));
    }

    /// Compile both SDKs with the real compilers and run a program that
    /// prints, so a broken SDK fails here rather than in a chat.
    #[tokio::test]
    async fn a_program_compiles_and_runs_when_the_compilers_are_there() {
        for (lang, code) in [(Lang::Rust, "fn main() { println!(\"hi {}\", json(\"a\\\"b\")); }"), (Lang::Cpp, "#include <iostream>\nint main() { std::cout << \"hi \" << viv::json(\"a\\\"b\") << \"\\n\"; }")] {
            if which::which(lang.compiler()).is_err() {
                continue;
            }
            let (exe, _) = compile(lang, code).await.unwrap_or_else(|e| panic!("{lang:?}: {e}"));
            let out = std::process::Command::new(&exe).output().unwrap();
            assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "hi \"a\\\"b\"", "{lang:?}");
        }
    }
}

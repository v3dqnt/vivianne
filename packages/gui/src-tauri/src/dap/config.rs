//! Which debug adapter runs which program, how it is started, and what it is asked to launch or attach to.
//!
//! Three sources of adapter definitions, in this order: the built-in table, the user's own `dap.json` in their Vivianne folder, and the
//! project's `.viv/dap.json` — **the last only once the user has trusted the project** (the same trust the project's mods and language
//! servers use). A cloned repository must not be able to say "the debugger for this is `./bin/dlv`" and have Vivianne run it. What a project
//! defines but is not trusted for is listed, with the full command, and not used. A built-in adapter's program is found on PATH only; a
//! user or trusted-project definition may also name a path. (oh-my-pi lets a repo-local binary beat the one on PATH with no trust check.)
//!
//! ```json
//! { "adapters": { "delve": { "args": ["dap", "--listen=127.0.0.1:{port}"], "launchDefaults": { "buildFlags": "-tags=dev" } },
//!                 "mine":  { "command": "my-dap", "extensions": ["foo"], "listen": false } } }
//! ```
//!
//! A field present in a definition replaces the built-in one; `launchDefaults` and `attachDefaults` merge key by key. `launchDefaults` never
//! overrides the program or directory the agent asked for. `env` values of `null` unset the variable (the protocol's meaning, which omp drops).

use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::Path;

/// Where an adapter definition came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Builtin,
    User,
    Project,
}

/// One debug adapter Vivianne can start.
#[derive(Debug, Clone)]
pub struct Adapter {
    pub name: String,
    /// Programs that may provide it, in order of preference (`python`, then `python3`).
    pub commands: Vec<String>,
    /// Arguments. `{port}` becomes a free loopback port for an adapter that `listen`s; `{js_debug_server}` is js-debug's script.
    pub args: Vec<String>,
    /// File extensions it debugs, lower case; `""` is a program with no extension (a native binary).
    pub extensions: Vec<String>,
    /// Files whose presence in the project makes this adapter the better fit among those that match.
    pub root_markers: Vec<String>,
    /// What to tell the user when it is not installed.
    pub hint: String,
    /// It listens on a TCP port instead of speaking on stdio (delve, js-debug, codelldb).
    pub listen: bool,
    /// It asks for a second connection (a child session) for each process it starts (js-debug).
    pub children: bool,
    /// It can debug a directory (a Go package).
    pub accepts_directory: bool,
    pub launch_defaults: Value,
    pub attach_defaults: Value,
    /// Variables set for the adapter; `None` unsets one.
    pub env: BTreeMap<String, Option<String>>,
    pub source: Source,
}

fn def(name: &str, commands: &[&str], args: &[&str], extensions: &[&str], markers: &[&str], hint: &str) -> Adapter {
    let strings = |s: &[&str]| s.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    Adapter {
        name: name.into(),
        commands: strings(commands),
        args: strings(args),
        extensions: strings(extensions),
        root_markers: strings(markers),
        hint: hint.into(),
        listen: false,
        children: false,
        accepts_directory: false,
        launch_defaults: json!({}),
        attach_defaults: json!({}),
        env: BTreeMap::new(),
        source: Source::Builtin,
    }
}

/// The adapters Vivianne ships. **None of these argument lists has been run against the real adapter in this repository's tests**: each one
/// needs a real launch before it is trusted.
pub fn builtin() -> Vec<Adapter> {
    let mut js = def("js-debug", &["node"], &["{js_debug_server}", "{port}", "127.0.0.1"], &["js", "mjs", "cjs"], &["package.json"], "install js-debug and set JS_DEBUG_DAP_SERVER to its dapDebugServer.js (or put it in ~/.local/opt/js-debug/src)");
    js.listen = true;
    js.children = true;
    let mut delve = def("delve", &["dlv"], &["dap", "--listen=127.0.0.1:{port}"], &["go"], &["go.mod", "go.work"], "install it with `go install github.com/go-delve/delve/cmd/dlv@latest`");
    delve.listen = true;
    delve.accepts_directory = true;
    // CodeLLDB serves DAP on a port it is given (omp starts it with `--port 0` and then talks stdio to it, which cannot work).
    let mut codelldb = def("codelldb", &["codelldb"], &["--port", "{port}"], &["c", "cc", "cpp", "cxx", "rs", "zig"], &["Cargo.toml", "CMakeLists.txt"], "install the CodeLLDB adapter (vadimcn/codelldb) and put `codelldb` on PATH");
    codelldb.listen = true;
    vec![
        def("debugpy", &["python", "python3"], &["-m", "debugpy.adapter"], &["py", "pyw"], &["pyproject.toml", "setup.py", "requirements.txt"], "install it with `pip install debugpy`"),
        def("lldb-dap", &["lldb-dap"], &[], &["rs", "c", "cc", "cpp", "cxx", "h", "hpp", "m", "mm", "swift", "zig", ""], &["Cargo.toml", "CMakeLists.txt", "Package.swift", "build.zig"], "it comes with LLVM"),
        def("gdb", &["gdb"], &["-i", "dap"], &["rs", "c", "cc", "cpp", "cxx", ""], &["Cargo.toml", "CMakeLists.txt", "Makefile"], "it needs GDB 14 or newer"),
        codelldb,
        def("netcoredbg", &["netcoredbg"], &["--interpreter=vscode"], &["cs", "fs", "dll"], &["global.json"], "install netcoredbg from Samsung/netcoredbg"),
        delve,
        js,
    ]
}

/// Which of several adapters for one file wins a tie, after root markers. oh-my-pi puts gdb first; here lldb-dap is first, because gdb's DAP
/// mode needs gdb 14 and an older gdb on PATH would be chosen over a working lldb-dap.
const RANK: &[&str] = &["lldb-dap", "gdb", "codelldb"];

/// An adapter and the command that starts it here.
#[derive(Debug, Clone)]
pub struct Resolved {
    pub adapter: Adapter,
    /// The program that was found.
    pub command: String,
    pub args: Vec<String>,
}

impl Resolved {
    /// The command line as it would be typed, for an approval card.
    pub fn line(&self) -> String {
        std::iter::once(self.command.as_str()).chain(self.args.iter().map(|a| a.as_str())).collect::<Vec<_>>().join(" ")
    }
}

/// Where js-debug's `dapDebugServer.js` is: `JS_DEBUG_DAP_SERVER`, then the places an editor plugin or a manual install puts it.
pub fn js_debug_server() -> Option<String> {
    let home = dirs::home_dir();
    let mut candidates: Vec<std::path::PathBuf> = Vec::new();
    if let Ok(p) = std::env::var("JS_DEBUG_DAP_SERVER") {
        candidates.push(p.into());
    }
    if let Some(h) = &home {
        candidates.push(h.join(".local/opt/js-debug/src/dapDebugServer.js"));
        candidates.push(h.join(".local/share/nvim/mason/packages/js-debug-adapter/js-debug/src/dapDebugServer.js"));
    }
    candidates.into_iter().find(|p| p.is_file()).map(|p| p.to_string_lossy().into_owned())
}

fn on_path(command: &str) -> Option<String> {
    which::which(command).ok().map(|p| p.display().to_string())
}

// ── Configuration files ─────────────────────────────────────────────────────

fn strings(v: &Value, key: &str) -> Option<Vec<String>> {
    v.get(key).and_then(|a| a.as_array()).map(|a| a.iter().filter_map(|s| s.as_str().map(String::from)).collect())
}

/// Merge `over` onto `base`, key by key.
fn merge_object(base: &mut Value, over: &Value) {
    if let (Some(b), Some(o)) = (base.as_object_mut(), over.as_object()) {
        for (k, v) in o {
            b.insert(k.clone(), v.clone());
        }
    }
}

/// Apply one config file to `adapters`. Returns what was wrong with it, or `Ok` with the command lines it defines (for the caller that
/// ignores it). `apply == false` only reads, to report what an untrusted project asks for.
pub fn apply_config(adapters: &mut Vec<Adapter>, text: &str, source: Source, apply: bool) -> Result<Vec<String>, String> {
    let doc: Value = serde_json::from_str(text).map_err(|e| format!("not valid JSON: {e}"))?;
    let map = doc.get("adapters").and_then(|a| a.as_object()).or_else(|| doc.as_object()).ok_or("not an object")?;
    let mut lines = Vec::new();
    for (name, entry) in map {
        let Some(entry) = entry.as_object().map(|_| entry) else { continue };
        let pos = adapters.iter().position(|a| &a.name == name);
        if entry.get("disabled").and_then(|d| d.as_bool()) == Some(true) {
            if apply {
                if let Some(i) = pos {
                    adapters.remove(i);
                }
            }
            continue;
        }
        let mut a = match pos {
            Some(i) => adapters[i].clone(),
            None => {
                if entry.get("command").and_then(|c| c.as_str()).is_none() {
                    // A definition of a new adapter has to say what to run.
                    continue;
                }
                let mut a = def(name, &[], &[], &[], &[], "");
                a.hint = format!("{name} is defined in a dap.json; check its command is installed");
                a
            }
        };
        if let Some(c) = entry.get("command").and_then(|c| c.as_str()) {
            a.commands = vec![c.to_string()];
        }
        if let Some(v) = strings(entry, "args") {
            a.args = v;
        }
        if let Some(v) = strings(entry, "extensions").or_else(|| strings(entry, "fileTypes")) {
            a.extensions = v.into_iter().map(|e| e.trim_start_matches('.').to_ascii_lowercase()).collect();
        }
        if let Some(v) = strings(entry, "rootMarkers") {
            a.root_markers = v;
        }
        if let Some(b) = entry.get("listen").and_then(|v| v.as_bool()) {
            a.listen = b;
        }
        if let Some(b) = entry.get("acceptsDirectoryProgram").and_then(|v| v.as_bool()) {
            a.accepts_directory = b;
        }
        if let Some(o) = entry.get("launchDefaults") {
            merge_object(&mut a.launch_defaults, o);
        }
        if let Some(o) = entry.get("attachDefaults") {
            merge_object(&mut a.attach_defaults, o);
        }
        if let Some(env) = entry.get("env").and_then(|e| e.as_object()) {
            for (k, v) in env {
                a.env.insert(k.clone(), v.as_str().map(String::from));
            }
        }
        a.source = source;
        lines.push(format!("{}: {}", a.name, std::iter::once(a.commands.first().cloned().unwrap_or_default()).chain(a.args.iter().cloned()).collect::<Vec<_>>().join(" ")));
        if apply {
            match pos {
                Some(i) => adapters[i] = a,
                None => adapters.push(a),
            }
        }
    }
    Ok(lines)
}

/// What the files said: the adapters to use, what an untrusted project defined and was not allowed, and files that could not be read.
#[derive(Debug, Default)]
pub struct Loaded {
    pub adapters: Vec<Adapter>,
    pub ignored: Vec<String>,
    pub problems: Vec<String>,
}

pub fn load_with(user: Option<&str>, project: Option<&str>, project_trusted: bool) -> Loaded {
    let mut out = Loaded { adapters: builtin(), ..Default::default() };
    if let Some(text) = user {
        if let Err(e) = apply_config(&mut out.adapters, text, Source::User, true) {
            out.problems.push(format!("your dap.json is {e}"));
        }
    }
    if let Some(text) = project {
        match apply_config(&mut out.adapters, text, Source::Project, project_trusted) {
            Ok(lines) if !project_trusted => out.ignored = lines,
            Ok(_) => {}
            Err(e) => out.problems.push(format!("the project's dap.json is {e}")),
        }
    }
    out
}

/// The adapters for work in `cwd`: the user's `dap.json` from their Vivianne folder and the project's `.viv/dap.json`.
pub fn load(cwd: &str) -> Loaded {
    let user = std::fs::read_to_string(crate::agent::system::paths::viv_home_path(&["dap.json"])).ok();
    let project = std::fs::read_to_string(Path::new(cwd).join(".viv").join("dap.json")).ok();
    let trusted = project.is_some() && crate::mods::project_trusted(cwd);
    load_with(user.as_deref(), project.as_deref(), trusted)
}

// ── Selection ───────────────────────────────────────────────────────────────

/// How adapters are looked up: programs by name, and the project's files.
pub struct Probe<'a> {
    /// The full path of a program found on PATH.
    pub on_path: &'a dyn Fn(&str) -> Option<String>,
    /// Whether the project (the working directory) has this file.
    pub has_file: &'a dyn Fn(&str) -> bool,
    /// Whether this path is a file (for a user or trusted-project definition that names its program by path).
    pub is_file: &'a dyn Fn(&str) -> bool,
    pub is_dir: bool,
    pub root: &'a str,
}

fn resolve(a: &Adapter, p: &Probe) -> Option<Resolved> {
    let command = a.commands.iter().find_map(|c| {
        // Only a definition the user wrote (or a project they trust) may name a path; a built-in is found on PATH alone.
        if a.source != Source::Builtin && (c.contains('/') || c.contains('\\')) {
            let full = if Path::new(c).is_absolute() { c.clone() } else { Path::new(p.root).join(c).to_string_lossy().into_owned() };
            return (p.is_file)(&full).then_some(full);
        }
        (p.on_path)(c)
    })?;
    let mut args = a.args.clone();
    if let Some(slot) = args.iter_mut().find(|x| x.as_str() == "{js_debug_server}") {
        *slot = js_debug_server()?;
    }
    Some(Resolved { adapter: a.clone(), command, args })
}

fn tie_break(a: &Adapter, p: &Probe) -> (bool, usize, String) {
    let marker = a.root_markers.iter().any(|m| (p.has_file)(m));
    (!marker, RANK.iter().position(|r| *r == a.name).unwrap_or(RANK.len()), a.name.clone())
}

/// The adapter for `program`, or the one named, with the command that starts it found through `probe`.
pub fn select_in(adapters: &[Adapter], program: &str, explicit: Option<&str>, probe: &Probe) -> Result<Resolved, String> {
    let names = || adapters.iter().map(|a| a.name.as_str()).collect::<Vec<_>>().join(", ");
    if let Some(name) = explicit.map(str::trim).filter(|n| !n.is_empty()) {
        let adapter = adapters.iter().find(|a| a.name == name).ok_or_else(|| format!("no debug adapter called '{name}' (there is {})", names()))?;
        if probe.is_dir && !adapter.accepts_directory {
            return Err(format!("{name} can't debug a directory: name a file"));
        }
        return resolve(adapter, probe).ok_or_else(|| format!("{name} isn't installed: {}", adapter.hint));
    }
    let mut candidates: Vec<&Adapter> = if probe.is_dir {
        let c: Vec<&Adapter> = adapters.iter().filter(|a| a.accepts_directory).collect();
        if c.is_empty() {
            return Err("no debug adapter here can debug a directory: name a file".into());
        }
        c
    } else {
        let ext = Path::new(program).extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).unwrap_or_default();
        let c: Vec<&Adapter> = adapters.iter().filter(|a| a.extensions.contains(&ext)).collect();
        if c.is_empty() {
            return Err(format!("no debug adapter is known for '{}' files (there is {}); name one with `adapter` if it can debug this", if ext.is_empty() { "extensionless" } else { &ext }, names()));
        }
        c
    };
    candidates.sort_by_key(|a| tie_break(a, probe));
    // The first that is installed; if none is, every candidate is named so the user knows what to install.
    candidates.iter().find_map(|a| resolve(a, probe)).ok_or_else(|| {
        let missing = candidates.iter().map(|a| format!("{} ({})", a.name, a.hint)).collect::<Vec<_>>().join("; ");
        format!("no debug adapter for this program is installed: {missing}")
    })
}

/// The adapter for attaching: the one named; else debugpy when a port is given; else gdb, then lldb-dap; else the first installed.
pub fn select_attach_in(adapters: &[Adapter], explicit: Option<&str>, port: Option<u16>, probe: &Probe) -> Result<Resolved, String> {
    if let Some(name) = explicit.map(str::trim).filter(|n| !n.is_empty()) {
        return select_in(adapters, "", Some(name), &Probe { is_dir: false, ..clone_probe(probe) });
    }
    let order: Vec<&str> = if port.is_some() { vec!["debugpy", "gdb", "lldb-dap"] } else { vec!["gdb", "lldb-dap"] };
    for name in order {
        if let Some(a) = adapters.iter().find(|a| a.name == name) {
            if let Some(r) = resolve(a, probe) {
                return Ok(r);
            }
        }
    }
    adapters.iter().find_map(|a| resolve(a, probe)).ok_or_else(|| "no debug adapter is installed to attach with".to_string())
}

fn clone_probe<'a>(p: &Probe<'a>) -> Probe<'a> {
    Probe { on_path: p.on_path, has_file: p.has_file, is_file: p.is_file, is_dir: p.is_dir, root: p.root }
}

fn with_probe<T>(cwd: &str, is_dir: bool, f: impl FnOnce(&Probe) -> T) -> T {
    let root = cwd.to_string();
    let has_file = |m: &str| Path::new(&root).join(m).exists();
    let is_file = |p: &str| Path::new(p).is_file();
    f(&Probe { on_path: &on_path, has_file: &has_file, is_file: &is_file, is_dir, root: cwd })
}

/// Choose the adapter for a launch in `cwd`.
pub fn select(program: &str, explicit: Option<&str>, cwd: &str) -> Result<Resolved, String> {
    let loaded = load(cwd);
    let is_dir = Path::new(program).is_dir();
    with_probe(cwd, is_dir, |p| select_in(&loaded.adapters, program, explicit, p))
}

/// Choose the adapter for an attach in `cwd`.
pub fn select_attach(explicit: Option<&str>, port: Option<u16>, cwd: &str) -> Result<Resolved, String> {
    let loaded = load(cwd);
    with_probe(cwd, false, |p| select_attach_in(&loaded.adapters, explicit, port, p))
}

// ── What the adapter is sent ────────────────────────────────────────────────

/// Put `defaults` under what the agent asked for: a default never replaces the program or directory, nor `args` when the agent gave some.
fn overlay(mut base: Value, defaults: &Value, keep: &[&str]) -> Value {
    if let (Some(b), Some(d)) = (base.as_object_mut(), defaults.as_object()) {
        for (k, v) in d {
            if !keep.contains(&k.as_str()) {
                b.insert(k.clone(), v.clone());
            }
        }
    }
    base
}

/// What `launch` is sent for this adapter: the program, where to run it, its arguments, and a stop at the start so the agent can set
/// breakpoints before anything runs past them. `is_dir` is whether `program` is a directory (a Go package).
pub fn launch_arguments(adapter: &Adapter, program: &str, cwd: &str, args: &[String], is_dir: bool) -> Value {
    let base = match adapter.name.as_str() {
        "debugpy" => json!({ "request": "launch", "type": "python", "program": program, "cwd": cwd, "args": args, "stopOnEntry": true, "justMyCode": false, "console": "internalConsole" }),
        "gdb" => json!({ "program": program, "cwd": cwd, "args": args, "stopAtBeginningOfMainSubprogram": true }),
        "netcoredbg" => json!({ "program": program, "cwd": cwd, "args": args, "stopAtEntry": true }),
        "js-debug" => json!({ "type": "pwa-node", "request": "launch", "program": program, "cwd": cwd, "args": args, "stopOnEntry": true, "console": "internalConsole" }),
        // `debug` builds and runs a package or a Go file; `exec` runs a binary that is already built.
        "delve" => json!({ "request": "launch", "mode": if is_dir || program.ends_with(".go") { "debug" } else { "exec" }, "program": program, "cwd": cwd, "args": args, "stopOnEntry": true }),
        "codelldb" => json!({ "program": program, "cwd": cwd, "args": args, "stopOnEntry": true }),
        _ => json!({ "program": program, "cwd": cwd, "args": args, "stopOnEntry": true }),
    };
    let keep: &[&str] = if args.is_empty() { &["program", "cwd"] } else { &["program", "cwd", "args"] };
    overlay(base, &adapter.launch_defaults, keep)
}

/// What `attach` is sent: a process id, or a host and port the program (or an adapter inside it) listens on.
pub fn attach_arguments(adapter: &Adapter, pid: Option<i64>, port: Option<u16>, host: &str, cwd: &str) -> Value {
    let mut v = match adapter.name.as_str() {
        "debugpy" => match port {
            Some(p) => json!({ "request": "attach", "connect": { "host": host, "port": p }, "justMyCode": false }),
            None => json!({ "request": "attach", "processId": pid }),
        },
        "gdb" => json!({ "pid": pid }),
        "delve" => match port {
            Some(p) => json!({ "request": "attach", "mode": "remote", "host": host, "port": p }),
            None => json!({ "request": "attach", "mode": "local", "processId": pid }),
        },
        "js-debug" => json!({ "type": "pwa-node", "request": "attach", "processId": pid, "port": port, "address": host }),
        _ => json!({ "pid": pid, "processId": pid }),
    };
    if adapter.name != "debugpy" && adapter.name != "delve" && adapter.name != "js-debug" {
        if let Some(p) = port {
            v["port"] = json!(p);
            v["host"] = json!(host);
        }
    }
    v["cwd"] = json!(cwd);
    overlay(v, &adapter.attach_defaults, &["cwd", "pid", "processId", "port"])
}

/// What an approval card says about a call to the run tool: for a launch or attach, the exact command that will be started.
pub fn describe_call(args: &Value, cwd: &str) -> String {
    let action = args.get("action").and_then(|a| a.as_str()).unwrap_or("");
    let adapter = args.get("adapter").and_then(|a| a.as_str());
    let loaded = load(cwd);
    let note = if loaded.ignored.is_empty() { String::new() } else { format!(" (this project's dap.json defines {}; not used until the project is trusted)", loaded.ignored.join("; ")) };
    if action == "attach" {
        let port = args.get("port").and_then(|p| p.as_u64()).map(|p| p as u16);
        let target = match (args.get("pid").and_then(|p| p.as_i64()), port) {
            (Some(pid), _) => format!("process {pid}"),
            (None, Some(p)) => format!("port {p}"),
            _ => "a process".to_string(),
        };
        return match with_probe(cwd, false, |p| select_attach_in(&loaded.adapters, adapter, port, p)) {
            Ok(r) => format!("starts the debug adapter `{}` and attaches it to {target}{note}", r.line()),
            Err(why) => format!("wants to attach to {target}, but no adapter can be started: {why}{note}"),
        };
    }
    let program = args.get("program").and_then(|p| p.as_str()).unwrap_or("?");
    let shown_args = args.get("args").and_then(|a| a.as_array()).map(|a| a.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>().join(" ")).unwrap_or_default();
    let is_dir = Path::new(program).is_dir();
    match with_probe(cwd, is_dir, |p| select_in(&loaded.adapters, program, adapter, p)) {
        Ok(r) => {
            let defaults = r.adapter.launch_defaults.as_object().filter(|o| !o.is_empty()).map(|o| format!(", with {} from a dap.json", o.keys().cloned().collect::<Vec<_>>().join(", "))).unwrap_or_default();
            format!("starts the debug adapter `{}` and runs {program}{}{defaults}{note}", r.line(), if shown_args.is_empty() { String::new() } else { format!(" with {shown_args}") })
        }
        Err(why) => format!("wants to debug {program}, but no adapter can be started: {why}{note}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn probe<'a>(on_path: &'a dyn Fn(&str) -> Option<String>, files: &'a dyn Fn(&str) -> bool, is_dir: bool) -> Probe<'a> {
        Probe { on_path, has_file: files, is_file: &|_| false, is_dir, root: "/w" }
    }

    fn installed(names: &'static [&'static str]) -> impl Fn(&str) -> Option<String> {
        move |c| names.contains(&c).then(|| format!("/bin/{c}"))
    }

    fn pick(program: &str, explicit: Option<&str>, names: &'static [&'static str], files: &'static [&'static str]) -> Result<Resolved, String> {
        let p = installed(names);
        let f = move |m: &str| files.contains(&m);
        select_in(&builtin(), program, explicit, &probe(&p, &f, false))
    }

    #[test]
    fn an_adapter_is_chosen_by_the_programs_extension_and_found_on_path() {
        let all: &[&str] = &["python3", "lldb-dap", "gdb", "netcoredbg", "dlv", "codelldb"];
        let py = pick("app/main.py", None, all, &[]).unwrap();
        assert_eq!((py.adapter.name.as_str(), py.command.as_str(), py.args.clone()), ("debugpy", "/bin/python3", vec!["-m".to_string(), "debugpy.adapter".to_string()]), "python3 when python is not there");
        assert_eq!(py.line(), "/bin/python3 -m debugpy.adapter");
        assert_eq!(pick("src/main.rs", None, all, &[]).unwrap().adapter.name, "lldb-dap", "the ranked first that is installed");
        assert_eq!(pick("src/main.rs", None, &["gdb"], &[]).unwrap().adapter.name, "gdb", "the next when it is not");
        assert_eq!(pick("target/debug/app", None, all, &[]).unwrap().adapter.name, "lldb-dap", "a program with no extension is a native binary");
        assert_eq!(pick("Program.CS", None, all, &[]).unwrap().adapter.name, "netcoredbg", "extensions are matched without case");
    }

    #[test]
    fn a_project_marker_breaks_a_tie_between_adapters_for_one_file() {
        // codelldb and lldb-dap both debug Rust; only codelldb's marker (here: its own, Cargo.toml is shared) decides when one has it alone.
        let mut adapters = builtin();
        adapters.iter_mut().find(|a| a.name == "lldb-dap").unwrap().root_markers = vec!["never-here".into()];
        let p = installed(&["lldb-dap", "codelldb"]);
        let f = |m: &str| m == "Cargo.toml";
        let r = select_in(&adapters, "src/main.rs", None, &probe(&p, &f, false)).unwrap();
        assert_eq!(r.adapter.name, "codelldb", "the one whose marker the project has");
    }

    #[test]
    fn what_cannot_be_started_says_why_and_what_to_do() {
        let e = pick("a.py", None, &[], &[]).unwrap_err();
        assert!(e.contains("debugpy") && e.contains("pip install debugpy"), "{e}");
        let e = pick("a.rs", None, &[], &[]).unwrap_err();
        assert!(e.contains("lldb-dap") && e.contains("gdb"), "{e}");
        assert!(pick("a.unknown", None, &["gdb"], &[]).unwrap_err().contains("no debug adapter is known"));
        assert!(pick("a.py", Some("nope"), &[], &[]).unwrap_err().contains("no debug adapter called 'nope'"));
        assert!(pick("a.py", Some("gdb"), &[], &[]).unwrap_err().contains("gdb isn't installed"));
    }

    #[test]
    fn naming_an_adapter_overrides_the_extension() {
        let all: &[&str] = &["gdb", "lldb-dap"];
        assert_eq!(pick("src/main.rs", Some("gdb"), all, &[]).unwrap().adapter.name, "gdb");
        assert_eq!(pick("anything", Some(" lldb-dap "), all, &[]).unwrap().adapter.name, "lldb-dap", "whitespace is ignored");
    }

    #[test]
    fn a_directory_goes_to_an_adapter_that_can_debug_one() {
        let p = installed(&["dlv", "gdb"]);
        let f = |_: &str| false;
        let r = select_in(&builtin(), "cmd/app", None, &probe(&p, &f, true)).unwrap();
        assert_eq!(r.adapter.name, "delve");
        assert!(select_in(&builtin(), "cmd/app", Some("gdb"), &probe(&p, &f, true)).unwrap_err().contains("can't debug a directory"));
        assert_eq!(launch_arguments(&r.adapter, "cmd/app", "/w", &[], true)["mode"], "debug");
        assert_eq!(launch_arguments(&r.adapter, "bin/app", "/w", &[], false)["mode"], "exec", "a built binary is run, not built");
        assert_eq!(launch_arguments(&r.adapter, "main.go", "/w", &[], false)["mode"], "debug");
    }

    #[test]
    fn go_programs_go_to_delve_on_a_port_vivianne_picks() {
        let r = pick("cmd/app/main.go", None, &["dlv"], &[]).unwrap();
        assert_eq!(r.adapter.name, "delve");
        assert!(r.adapter.listen && r.args.iter().any(|a| a.contains("{port}")));
    }

    #[test]
    fn attaching_prefers_debugpy_for_a_port_and_gdb_for_a_process() {
        let p = installed(&["python3", "gdb", "lldb-dap"]);
        let f = |_: &str| false;
        let pr = probe(&p, &f, false);
        assert_eq!(select_attach_in(&builtin(), None, Some(5678), &pr).unwrap().adapter.name, "debugpy");
        assert_eq!(select_attach_in(&builtin(), None, None, &pr).unwrap().adapter.name, "gdb");
        assert_eq!(select_attach_in(&builtin(), Some("lldb-dap"), None, &pr).unwrap().adapter.name, "lldb-dap");
        let d = builtin().into_iter().find(|a| a.name == "debugpy").unwrap();
        assert_eq!(attach_arguments(&d, None, Some(5678), "127.0.0.1", "/w")["connect"]["port"], 5678);
        let g = builtin().into_iter().find(|a| a.name == "gdb").unwrap();
        assert_eq!(attach_arguments(&g, Some(42), None, "127.0.0.1", "/w")["pid"], 42);
    }

    #[test]
    fn a_launch_stops_at_the_start_the_way_each_adapter_names_it() {
        let by = |n: &str| builtin().into_iter().find(|a| a.name == n).unwrap();
        let args = vec!["--fast".to_string()];
        let py = launch_arguments(&by("debugpy"), "a.py", "/w", &args, false);
        assert_eq!((py["program"].clone(), py["cwd"].clone(), py["args"].clone(), py["stopOnEntry"].clone()), (json!("a.py"), json!("/w"), json!(["--fast"]), json!(true)));
        assert_eq!(py["justMyCode"], false, "the agent wants to see library code too");
        assert_eq!(launch_arguments(&by("gdb"), "a.out", "/w", &[], false)["stopAtBeginningOfMainSubprogram"], true);
        assert_eq!(launch_arguments(&by("netcoredbg"), "a.dll", "/w", &[], false)["stopAtEntry"], true);
        assert_eq!(launch_arguments(&by("lldb-dap"), "a.out", "/w", &[], false)["stopOnEntry"], true);
    }

    // ── configuration files ──

    #[test]
    fn a_projects_adapters_are_ignored_until_the_project_is_trusted_and_shown_in_full() {
        let cfg = r#"{ "adapters": { "gdb": { "command": "./evil/gdb", "args": ["--x"] } } }"#;
        let untrusted = load_with(None, Some(cfg), false);
        assert_eq!(untrusted.adapters.iter().find(|a| a.name == "gdb").unwrap().commands, vec!["gdb".to_string()], "the built-in stands");
        assert_eq!(untrusted.ignored, vec!["gdb: ./evil/gdb --x".to_string()]);
        let trusted = load_with(None, Some(cfg), true);
        let g = trusted.adapters.iter().find(|a| a.name == "gdb").unwrap();
        assert_eq!((g.commands.clone(), g.args.clone(), g.source), (vec!["./evil/gdb".to_string()], vec!["--x".to_string()], Source::Project));
        assert!(trusted.ignored.is_empty());
    }

    #[test]
    fn a_built_in_adapter_is_found_on_path_alone_but_a_trusted_definition_may_name_a_path() {
        let user = r#"{ "adapters": { "mine": { "command": "tools/my-dap", "extensions": [".foo"] }, "gdb": { "command": "/opt/gdb14/bin/gdb" } } }"#;
        let l = load_with(Some(user), None, false);
        let path = |_: &str| None;
        let f = |_: &str| false;
        let is_file = |p: &str| p == "/w/tools/my-dap" || p == "/opt/gdb14/bin/gdb";
        let pr = Probe { on_path: &path, has_file: &f, is_file: &is_file, is_dir: false, root: "/w" };
        assert_eq!(select_in(&l.adapters, "x.foo", None, &pr).unwrap().command, "/w/tools/my-dap", "a path is joined to the project");
        assert_eq!(select_in(&l.adapters, "x", Some("gdb"), &pr).unwrap().command, "/opt/gdb14/bin/gdb", "the user's own gdb");
        // The same text, for a built-in that the user did not override, never takes a path.
        let plain = Probe { on_path: &|c| Some(format!("/usr/bin/{c}")), ..clone_probe(&pr) };
        assert_eq!(select_in(&builtin(), "a.py", None, &plain).unwrap().command, "/usr/bin/python");
    }

    #[test]
    fn defaults_merge_key_by_key_and_never_replace_the_program() {
        let user = r#"{ "adapters": { "delve": { "launchDefaults": { "buildFlags": "-tags=dev", "program": "/evil", "cwd": "/evil", "args": ["--from-config"] } } } }"#;
        let l = load_with(Some(user), None, false);
        let d = l.adapters.iter().find(|a| a.name == "delve").unwrap();
        let v = launch_arguments(d, "main.go", "/w", &[], false);
        assert_eq!((v["buildFlags"].clone(), v["program"].clone(), v["cwd"].clone(), v["stopOnEntry"].clone()), (json!("-tags=dev"), json!("main.go"), json!("/w"), json!(true)));
        assert_eq!(v["args"], json!(["--from-config"]), "a default `args` stands when the agent gave none");
        assert_eq!(launch_arguments(d, "main.go", "/w", &["-v".to_string()], false)["args"], json!(["-v"]), "and yields when it did");
    }

    #[test]
    fn env_null_unsets_and_a_broken_file_does_not_stop_the_rest() {
        let l = load_with(Some(r#"{ "adapters": { "gdb": { "env": { "A": "1", "SECRET": null } } } }"#), None, false);
        let g = l.adapters.iter().find(|a| a.name == "gdb").unwrap();
        assert_eq!((g.env.get("A").cloned(), g.env.get("SECRET").cloned()), (Some(Some("1".to_string())), Some(None)), "null is kept as `unset`");
        let bad = load_with(Some("{ nope"), None, false);
        assert!(bad.problems[0].contains("dap.json") && bad.adapters.len() == builtin().len());
        // A new adapter with no command is not one.
        let none = load_with(Some(r#"{ "adapters": { "ghost": { "extensions": ["x"] } } }"#), None, false);
        assert!(none.adapters.iter().all(|a| a.name != "ghost"));
        // Disabling removes it.
        let off = load_with(Some(r#"{ "adapters": { "gdb": { "disabled": true } } }"#), None, false);
        assert!(off.adapters.iter().all(|a| a.name != "gdb"));
    }
}

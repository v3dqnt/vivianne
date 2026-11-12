//! Which debug adapter runs which program, how it is started, and what it is asked to launch.
//!
//! A short, fixed table, not read from the project. A project's own files may not name a command Vivianne runs: a cloned repository that could
//! say "the debugger for this is `./bin/dlv`" would run its own binary the moment the user approved a launch of a file in it. Adapters are
//! found on PATH only. (oh-my-pi reads adapter definitions from the project and lets a repo-local binary beat the one on PATH; this does not.)
//!
//! Adapters that speak DAP on stdio, and delve, which listens on a loopback port Vivianne picks and connects to. js-debug (a second
//! connection per child session) is not here yet.

use serde_json::{json, Value};

/// One debug adapter Vivianne knows how to start.
#[derive(Debug)]
pub struct Adapter {
    pub name: &'static str,
    /// Programs that may provide it, in order of preference (`python`, then `python3`).
    pub commands: &'static [&'static str],
    pub args: &'static [&'static str],
    /// File extensions it debugs, lower case; `""` is a program with no extension (a native binary).
    pub extensions: &'static [&'static str],
    /// What to tell the user when it is not installed.
    pub hint: &'static str,
    /// It listens on a TCP port instead of speaking on stdio (delve). `{port}` in `args` is replaced by a free port chosen at start.
    pub listen: bool,
}

pub const ADAPTERS: &[Adapter] = &[
    Adapter { name: "debugpy", commands: &["python", "python3"], args: &["-m", "debugpy.adapter"], extensions: &["py", "pyw"], hint: "install it with `pip install debugpy`", listen: false },
    Adapter { name: "lldb-dap", commands: &["lldb-dap"], args: &[], extensions: &["rs", "c", "cc", "cpp", "cxx", "h", "hpp", "m", "mm", "swift", "zig", ""], hint: "it comes with LLVM", listen: false },
    Adapter { name: "gdb", commands: &["gdb"], args: &["-i", "dap"], extensions: &["rs", "c", "cc", "cpp", "cxx", ""], hint: "it needs GDB 14 or newer", listen: false },
    Adapter { name: "netcoredbg", commands: &["netcoredbg"], args: &["--interpreter=vscode"], extensions: &["cs", "fs", "dll"], hint: "install netcoredbg from Samsung/netcoredbg", listen: false },
    Adapter { name: "delve", commands: &["dlv"], args: &["dap", "--listen=127.0.0.1:{port}"], extensions: &["go"], hint: "install it with `go install github.com/go-delve/delve/cmd/dlv@latest`", listen: true },
];

/// An adapter and the command that starts it here.
#[derive(Debug)]
pub struct Resolved {
    pub adapter: &'static Adapter,
    /// The full path of the program found on PATH.
    pub command: String,
    pub args: Vec<String>,
}

impl Resolved {
    /// The command line as it would be typed, for an approval card.
    pub fn line(&self) -> String {
        std::iter::once(self.command.as_str()).chain(self.args.iter().map(|a| a.as_str())).collect::<Vec<_>>().join(" ")
    }
}

fn on_path(command: &str) -> Option<String> {
    which::which(command).ok().map(|p| p.display().to_string())
}

/// The adapter for `program`, or the one named, with the command that starts it found through `lookup`.
pub fn select_with(program: &str, explicit: Option<&str>, lookup: &dyn Fn(&str) -> Option<String>) -> Result<Resolved, String> {
    let resolve = |a: &'static Adapter| a.commands.iter().find_map(|c| lookup(c)).map(|command| Resolved { adapter: a, command, args: a.args.iter().map(|s| s.to_string()).collect() });
    if let Some(name) = explicit.map(str::trim).filter(|n| !n.is_empty()) {
        let adapter = ADAPTERS.iter().find(|a| a.name == name).ok_or_else(|| format!("no debug adapter called '{name}' (there is {})", ADAPTERS.iter().map(|a| a.name).collect::<Vec<_>>().join(", ")))?;
        return resolve(adapter).ok_or_else(|| format!("{name} isn't installed: {}", adapter.hint));
    }
    let ext = std::path::Path::new(program).extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).unwrap_or_default();
    let candidates: Vec<&'static Adapter> = ADAPTERS.iter().filter(|a| a.extensions.contains(&ext.as_str())).collect();
    if candidates.is_empty() {
        return Err(format!("no debug adapter is known for '{}' files (there is {}); name one with `adapter` if it can debug this", if ext.is_empty() { "extensionless" } else { &ext }, ADAPTERS.iter().map(|a| a.name).collect::<Vec<_>>().join(", ")));
    }
    candidates.iter().find_map(|a| resolve(a)).ok_or_else(|| {
        let missing = candidates.iter().map(|a| format!("{} ({})", a.name, a.hint)).collect::<Vec<_>>().join("; ");
        format!("no debug adapter for this program is installed: {missing}")
    })
}

pub fn select(program: &str, explicit: Option<&str>) -> Result<Resolved, String> {
    select_with(program, explicit, &on_path)
}

/// What `launch` is sent for this adapter: the program, where to run it, its arguments, and a stop at the start so the agent can set
/// breakpoints before anything runs past them.
pub fn launch_arguments(adapter: &Adapter, program: &str, cwd: &str, args: &[String]) -> Value {
    match adapter.name {
        "debugpy" => json!({ "request": "launch", "type": "python", "program": program, "cwd": cwd, "args": args, "stopOnEntry": true, "justMyCode": false, "console": "internalConsole" }),
        "gdb" => json!({ "program": program, "cwd": cwd, "args": args, "stopAtBeginningOfMainSubprogram": true }),
        "netcoredbg" => json!({ "program": program, "cwd": cwd, "args": args, "stopAtEntry": true }),
        "delve" => json!({ "request": "launch", "mode": "debug", "program": program, "cwd": cwd, "args": args, "stopOnEntry": true }),
        _ => json!({ "program": program, "cwd": cwd, "args": args, "stopOnEntry": true }),
    }
}

/// What an approval card says about a call to the run tool: for a launch, the exact command that will be started and the program it runs.
pub fn describe_call(args: &Value) -> String {
    let program = args.get("program").and_then(|p| p.as_str()).unwrap_or("?");
    let shown_args = args.get("args").and_then(|a| a.as_array()).map(|a| a.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>().join(" ")).unwrap_or_default();
    let adapter = args.get("adapter").and_then(|a| a.as_str());
    match select(program, adapter) {
        Ok(r) => format!("starts the debug adapter `{}` and runs {program}{}", r.line(), if shown_args.is_empty() { String::new() } else { format!(" with {shown_args}") }),
        Err(why) => format!("wants to debug {program}, but no adapter can be started: {why}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn installed(names: &'static [&'static str]) -> impl Fn(&str) -> Option<String> {
        move |c| names.contains(&c).then(|| format!("/bin/{c}"))
    }

    #[test]
    fn an_adapter_is_chosen_by_the_programs_extension_and_found_on_path() {
        let all = installed(&["python3", "lldb-dap", "gdb", "netcoredbg"]);
        let py = select_with("app/main.py", None, &all).unwrap();
        assert_eq!((py.adapter.name, py.command.as_str(), py.args.clone()), ("debugpy", "/bin/python3", vec!["-m".to_string(), "debugpy.adapter".to_string()]), "python3 when python is not there");
        assert_eq!(py.line(), "/bin/python3 -m debugpy.adapter");
        assert_eq!(select_with("src/main.rs", None, &all).unwrap().adapter.name, "lldb-dap", "the first that is installed");
        assert_eq!(select_with("src/main.rs", None, &installed(&["gdb"])).unwrap().adapter.name, "gdb", "the next when it is not");
        assert_eq!(select_with("target/debug/app", None, &all).unwrap().adapter.name, "lldb-dap", "a program with no extension is a native binary");
        assert_eq!(select_with("Program.CS", None, &all).unwrap().adapter.name, "netcoredbg", "extensions are matched without case");
    }

    #[test]
    fn what_cannot_be_started_says_why_and_what_to_do() {
        let none = installed(&[]);
        let e = select_with("a.py", None, &none).unwrap_err();
        assert!(e.contains("debugpy") && e.contains("pip install debugpy"), "{e}");
        let e = select_with("a.rs", None, &none).unwrap_err();
        assert!(e.contains("lldb-dap") && e.contains("gdb"), "{e}");
        let e = select_with("a.unknown", None, &installed(&["gdb"])).unwrap_err();
        assert!(e.contains("no debug adapter is known"), "{e}");
        assert!(select_with("a.py", Some("nope"), &none).unwrap_err().contains("no debug adapter called 'nope'"));
        assert!(select_with("a.py", Some("gdb"), &none).unwrap_err().contains("gdb isn't installed"));
    }

    #[test]
    fn naming_an_adapter_overrides_the_extension() {
        let all = installed(&["gdb", "lldb-dap"]);
        assert_eq!(select_with("src/main.rs", Some("gdb"), &all).unwrap().adapter.name, "gdb");
        assert_eq!(select_with("anything", Some(" lldb-dap "), &all).unwrap().adapter.name, "lldb-dap", "whitespace is ignored");
    }

    #[test]
    fn a_launch_stops_at_the_start_the_way_each_adapter_names_it() {
        let args = vec!["--fast".to_string()];
        let py = launch_arguments(&ADAPTERS[0], "a.py", "/w", &args);
        assert_eq!((py["program"].clone(), py["cwd"].clone(), py["args"].clone(), py["stopOnEntry"].clone()), (json!("a.py"), json!("/w"), json!(["--fast"]), json!(true)));
        assert_eq!(py["justMyCode"], false, "the agent wants to see library code too");
        assert_eq!(launch_arguments(&ADAPTERS[2], "a.out", "/w", &[])["stopAtBeginningOfMainSubprogram"], true);
        assert_eq!(launch_arguments(&ADAPTERS[3], "a.dll", "/w", &[])["stopAtEntry"], true);
        assert_eq!(launch_arguments(&ADAPTERS[1], "a.out", "/w", &[])["stopOnEntry"], true);
    }

    #[test]
    fn a_project_cannot_add_an_adapter() {
        // The table is the only source: nothing here reads a file from the project.
        assert!(ADAPTERS.iter().all(|a| !a.commands.iter().any(|c| c.contains('/') || c.contains('\\') || c.starts_with('.'))), "commands are bare names found on PATH");
    }

    #[test]
    fn go_programs_go_to_delve_on_a_port_vivianne_picks() {
        let r = select_with("cmd/app/main.go", None, &installed(&["dlv"])).unwrap();
        assert_eq!(r.adapter.name, "delve");
        assert!(r.adapter.listen && r.args.iter().any(|a| a.contains("{port}")));
        assert_eq!(launch_arguments(r.adapter, "main.go", "/w", &[])["mode"], "debug");
    }
}

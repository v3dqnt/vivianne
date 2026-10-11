//! Tools the agent writes for itself.
//!
//! When a task needs something the built-in tools don't do (convert a file
//! format, query an API the same way every time, run a fit the user keeps
//! asking for), the agent writes it once in Python with `create_tool` and
//! it becomes a real tool: in the tool list of every model the user runs,
//! from the next step on, in every project.
//!
//! One folder per tool in `~/.viv/tools/<name>/`:
//! - `tool.json`: name, description, JSON-schema parameters, and usage
//!   (calls, last used), which the shredder reads.
//! - `tool.py`: a module with `def run(**args)`. It returns a string, or
//!   anything JSON-serialisable, and prints whatever it likes to stderr.
//!
//! Tools pile up. Past `SOFT_LIMIT`, `create_tool` says so and the agent
//! reviews the set with `list_custom_tools`, retiring stale or duplicate
//! ones with `delete_tool`. Settings → Mods can also start that review (the
//! "shredder"). Retired tools go to `~/.viv/tools/.retired/`, not the void.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::tool_exec::{Tool, ToolResult};

/// Past this many, creating another asks for a review of the set.
pub const SOFT_LIMIT: usize = 30;
const DEFAULT_TIMEOUT_SECS: u64 = 120;
const MAX_TIMEOUT_SECS: u64 = 600;
const MAX_OUTPUT_CHARS: usize = 60_000;

/// Loads the tool module, calls `run(**args)` with the JSON on stdin, and
/// prints the result.
const RUNNER: &str = r#"import importlib.util, json, sys, traceback
spec = importlib.util.spec_from_file_location("viv_tool", sys.argv[1])
mod = importlib.util.module_from_spec(spec)
try:
    spec.loader.exec_module(mod)
except Exception:
    traceback.print_exc()
    sys.exit(3)
if not hasattr(mod, "run"):
    print("tool.py defines no run(**args) function", file=sys.stderr)
    sys.exit(3)
raw = sys.stdin.read()
args = json.loads(raw) if raw.strip() else {}
try:
    result = mod.run(**args)
except Exception:
    traceback.print_exc()
    sys.exit(1)
if result is None:
    pass
elif isinstance(result, str):
    print(result)
else:
    print(json.dumps(result, indent=2, default=str, ensure_ascii=False))
"#;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomToolDef {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
    /// The model that wrote it, for the review.
    #[serde(default)]
    pub created_by: Option<String>,
    #[serde(default)]
    pub uses: u64,
    #[serde(default)]
    pub last_used: Option<String>,
    #[serde(default)]
    pub timeout_secs: Option<u64>,
}

pub fn tools_dir() -> Result<PathBuf, String> {
    let dir = crate::agent::paths::viv_home()?.join("tools");
    std::fs::create_dir_all(&dir).map_err(|e| format!("Couldn't create the tools folder: {e}"))?;
    Ok(dir)
}

fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

/// `snake_case`, 3–48 characters, not a built-in's name or an MCP one.
fn check_name(name: &str) -> Result<(), String> {
    let ok = name.len() >= 3
        && name.len() <= 48
        && name.starts_with(|c: char| c.is_ascii_lowercase())
        && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    if !ok {
        return Err(format!("'{name}' isn't a usable tool name: use snake_case, 3 to 48 characters, starting with a letter."));
    }
    if name.starts_with("mcp__") {
        return Err("Names starting with mcp__ belong to MCP servers.".into());
    }
    if crate::agent::tool_schemas::reserved_tool_names().contains(name) || BUILTIN_NAMES.contains(&name) {
        return Err(format!("'{name}' is already a built-in tool. Pick a name that says what yours does differently."));
    }
    if crate::mods::tools::names().contains(name) {
        return Err(format!("'{name}' is already a mod's tool."));
    }
    Ok(())
}

/// The management tools themselves, which live in the static schema list
/// too; listed so a tool can't be saved under their names in a build whose
/// schema list is being assembled.
const BUILTIN_NAMES: &[&str] = &["create_tool", "delete_tool", "list_custom_tools"];

fn read_def(dir: &Path) -> Option<CustomToolDef> {
    let raw = std::fs::read_to_string(dir.join("tool.json")).ok()?;
    let def: CustomToolDef = serde_json::from_str(&raw).ok()?;
    dir.join("tool.py").is_file().then_some(def)
}

fn write_def(dir: &Path, def: &CustomToolDef) -> Result<(), String> {
    let json = serde_json::to_string_pretty(def).map_err(|e| e.to_string())?;
    std::fs::write(dir.join("tool.json"), json).map_err(|e| format!("Couldn't save tool.json: {e}"))
}

/// Every saved tool whose name is still free, sorted by name so the tool
/// list serialises the same way every time (prompt caches depend on it).
pub fn load_custom_tools() -> Vec<CustomToolDef> {
    let Ok(dir) = tools_dir() else { return Vec::new() };
    let Ok(entries) = std::fs::read_dir(&dir) else { return Vec::new() };
    let reserved = crate::agent::tool_schemas::reserved_tool_names();
    let mut defs: Vec<CustomToolDef> = entries
        .flatten()
        .filter(|e| e.path().is_dir() && !e.file_name().to_string_lossy().starts_with('.'))
        .filter_map(|e| read_def(&e.path()))
        .filter(|d| {
            let clash = reserved.contains(&d.name);
            if clash {
                log::warn!("[custom_tools] skipping '{}': a built-in tool now has that name", d.name);
            }
            !clash
        })
        .collect();
    defs.sort_by(|a, b| a.name.cmp(&b.name));
    defs
}

pub fn custom_tool_names() -> std::collections::HashSet<String> {
    load_custom_tools().into_iter().map(|d| d.name).collect()
}

/// Schema entries for the model's tool list.
pub fn tool_definitions() -> Vec<serde_json::Value> {
    load_custom_tools()
        .into_iter()
        .map(|d| {
            serde_json::json!({
                "name": d.name,
                "description": format!("[Your tool] {}", d.description),
                "parameters": d.parameters,
            })
        })
        .collect()
}

fn record_use(name: &str) {
    let Ok(dir) = tools_dir() else { return };
    let dir = dir.join(name);
    if let Some(mut def) = read_def(&dir) {
        def.uses += 1;
        def.last_used = Some(now());
        let _ = write_def(&dir, &def);
    }
}

fn clip(mut s: String) -> String {
    if s.len() > MAX_OUTPUT_CHARS {
        let mut end = MAX_OUTPUT_CHARS;
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        s.truncate(end);
        s.push_str("\n… (output truncated)");
    }
    s
}

/// Run `tool.py` in `dir` with `args`, from `cwd`.
async fn run_python(dir: &Path, args: &serde_json::Value, cwd: &str, timeout: Duration) -> ToolResult {
    let Some(python) = super::checkers::python() else {
        return ToolResult {
            content: "No Python found on PATH (the Microsoft Store stub doesn't count). Install Python 3 from python.org to use custom tools.".into(),
            is_error: true,
            details: None,
        };
    };
    let runner = dir.join(".runner.py");
    if let Err(e) = std::fs::write(&runner, RUNNER) {
        return ToolResult { content: format!("Couldn't prepare the tool runner: {e}"), is_error: true, details: None };
    }
    let mut cmd = tokio::process::Command::new(python);
    cmd.arg("-X").arg("utf8").arg(&runner).arg(dir.join("tool.py"));
    let workdir = if Path::new(cwd).is_dir() { cwd.to_string() } else { dir.to_string_lossy().into_owned() };
    cmd.current_dir(workdir).kill_on_drop(true);
    cmd.env_clear().envs(super::sandbox::filtered_env()).env("PYTHONIOENCODING", "utf-8");
    cmd.stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return ToolResult { content: format!("Couldn't start Python: {e}"), is_error: true, details: None },
    };
    if let Some(mut stdin) = child.stdin.take() {
        use tokio::io::AsyncWriteExt;
        let _ = stdin.write_all(args.to_string().as_bytes()).await;
    }
    match tokio::time::timeout(timeout, child.wait_with_output()).await {
        Err(_) => ToolResult {
            content: format!("Timed out after {}s and was stopped.", timeout.as_secs()),
            is_error: true,
            details: None,
        },
        Ok(Err(e)) => ToolResult { content: format!("The tool failed to run: {e}"), is_error: true, details: None },
        Ok(Ok(out)) => {
            let stdout = String::from_utf8_lossy(&out.stdout).trim_end().to_string();
            let stderr = String::from_utf8_lossy(&out.stderr).trim_end().to_string();
            let ok = out.status.success();
            let mut text = stdout;
            if !stderr.is_empty() {
                if !text.is_empty() {
                    text.push_str("\n\n");
                }
                text.push_str(if ok { "stderr:\n" } else { "Error:\n" });
                text.push_str(&stderr);
            }
            if text.is_empty() {
                text = if ok { "(no output)".into() } else { format!("Exited with {}", out.status) };
            }
            ToolResult { content: clip(text), is_error: !ok, details: None }
        }
    }
}

/// A saved tool, callable by its own name.
pub struct CustomTool {
    def: CustomToolDef,
    cwd: String,
}

#[async_trait]
impl Tool for CustomTool {
    fn name(&self) -> &str {
        &self.def.name
    }

    async fn execute(&self, args: &serde_json::Value, _cwd: &str) -> ToolResult {
        let dir = match tools_dir() {
            Ok(d) => d.join(&self.def.name),
            Err(e) => return ToolResult { content: e, is_error: true, details: None },
        };
        record_use(&self.def.name);
        let secs = self.def.timeout_secs.unwrap_or(DEFAULT_TIMEOUT_SECS).clamp(1, MAX_TIMEOUT_SECS);
        run_python(&dir, args, &self.cwd, Duration::from_secs(secs)).await
    }
}

pub fn registry(cwd: &str) -> Vec<Box<dyn Tool>> {
    load_custom_tools()
        .into_iter()
        .map(|def| Box::new(CustomTool { def, cwd: cwd.to_string() }) as Box<dyn Tool>)
        .collect()
}

fn err(msg: impl Into<String>) -> ToolResult {
    ToolResult { content: msg.into(), is_error: true, details: None }
}

/// Write a tool, test it when asked, and save it only if the test passes.
pub struct CreateToolTool {
    cwd: String,
}

impl CreateToolTool {
    pub fn new(cwd: String) -> Self {
        Self { cwd }
    }
}

#[async_trait]
impl Tool for CreateToolTool {
    fn name(&self) -> &str {
        "create_tool"
    }

    async fn execute(&self, args: &serde_json::Value, _cwd: &str) -> ToolResult {
        let s = |k: &str| args.get(k).and_then(|v| v.as_str()).map(str::trim).unwrap_or("").to_string();
        let name = s("name");
        let description = s("description");
        let code = args.get("code").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let replace = args.get("replace").and_then(|v| v.as_bool()).unwrap_or(false);
        let mut parameters = args.get("parameters").cloned().unwrap_or_else(|| serde_json::json!({"type": "object", "properties": {}}));
        if let serde_json::Value::String(raw) = &parameters {
            parameters = serde_json::from_str(raw).unwrap_or(serde_json::json!({"type": "object", "properties": {}}));
        }

        if let Err(e) = check_name(&name) {
            return err(e);
        }
        if description.len() < 10 {
            return err("Give the tool a description that says what it does and when to use it: other models choose it by this text alone.");
        }
        if !code.contains("def run") {
            return err("The code must define `def run(**args)` (or `def run(param_a, param_b=...)`), returning a string or anything JSON-serialisable.");
        }
        if parameters.get("type").and_then(|t| t.as_str()) != Some("object") {
            return err("`parameters` must be a JSON schema with \"type\": \"object\".");
        }

        let root = match tools_dir() {
            Ok(d) => d,
            Err(e) => return err(e),
        };
        let dest = root.join(&name);
        let existing = read_def(&dest);
        if existing.is_some() && !replace {
            return err(format!("A tool named '{name}' already exists. Pass replace: true to update it, or pick another name."));
        }

        // Built in a staging folder, so a failed test never leaves a broken
        // tool in everyone's tool list.
        let staging = root.join(format!(".staging-{name}"));
        let _ = std::fs::remove_dir_all(&staging);
        if let Err(e) = std::fs::create_dir_all(&staging).and_then(|_| std::fs::write(staging.join("tool.py"), &code)) {
            return err(format!("Couldn't write the tool: {e}"));
        }

        let mut test_report = String::new();
        if let Some(test_args) = args.get("test_args").filter(|v| v.is_object()) {
            let result = run_python(&staging, test_args, &self.cwd, Duration::from_secs(DEFAULT_TIMEOUT_SECS)).await;
            if result.is_error {
                let _ = std::fs::remove_dir_all(&staging);
                return err(format!("The test run failed, so the tool was not saved. Fix it and call create_tool again.\n\n{}", result.content));
            }
            test_report = format!("\n\nTest run with {test_args}:\n{}", result.content);
        }

        let stamp = now();
        let def = CustomToolDef {
            name: name.clone(),
            description,
            parameters,
            created_at: existing.as_ref().map(|d| d.created_at.clone()).unwrap_or_else(|| stamp.clone()),
            updated_at: stamp,
            created_by: args.get("_model").and_then(|v| v.as_str()).map(String::from),
            uses: existing.as_ref().map(|d| d.uses).unwrap_or(0),
            last_used: existing.as_ref().and_then(|d| d.last_used.clone()),
            timeout_secs: args.get("timeout_secs").and_then(|v| v.as_u64()).map(|t| t.clamp(1, MAX_TIMEOUT_SECS)),
        };
        if let Err(e) = write_def(&staging, &def) {
            let _ = std::fs::remove_dir_all(&staging);
            return err(e);
        }
        let _ = std::fs::remove_file(staging.join(".runner.py"));
        let _ = std::fs::remove_dir_all(&dest);
        if let Err(e) = std::fs::rename(&staging, &dest) {
            return err(format!("Couldn't save the tool: {e}"));
        }
        crate::agent::tool_exec::invalidate_tool_defs_cache();

        let count = load_custom_tools().len();
        let mut text = format!(
            "{} tool '{name}'. It's in your tool list from your next step, and in every model's, in every project.{test_report}",
            if existing.is_some() { "Updated" } else { "Saved" }
        );
        if count > SOFT_LIMIT {
            text.push_str(&format!(
                "\n\nThere are now {count} custom tools (more than {SOFT_LIMIT}). When this task is done, review them: call list_custom_tools, \
                 and retire with delete_tool the ones that are unused for a long time, duplicated by another tool, or superseded."
            ));
        }
        ToolResult { content: text, is_error: false, details: Some(serde_json::json!({ "name": name, "count": count })) }
    }
}

/// Move a tool to `.retired/`, out of every tool list.
pub fn retire(name: &str) -> Result<(), String> {
    let root = tools_dir()?;
    let dir = root.join(name);
    if read_def(&dir).is_none() {
        return Err(format!("There's no custom tool named '{name}'."));
    }
    let retired = root.join(".retired");
    std::fs::create_dir_all(&retired).map_err(|e| e.to_string())?;
    let target = retired.join(format!("{name}-{}", chrono::Utc::now().format("%Y%m%d-%H%M%S")));
    std::fs::rename(&dir, &target).map_err(|e| format!("Couldn't retire '{name}': {e}"))?;
    crate::agent::tool_exec::invalidate_tool_defs_cache();
    Ok(())
}

pub struct DeleteToolTool;

#[async_trait]
impl Tool for DeleteToolTool {
    fn name(&self) -> &str {
        "delete_tool"
    }

    async fn execute(&self, args: &serde_json::Value, _cwd: &str) -> ToolResult {
        let name = args.get("name").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
        let reason = args.get("reason").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
        if reason.is_empty() {
            return err("Say why in `reason`: the user sees it.");
        }
        match retire(&name) {
            Ok(()) => ToolResult {
                content: format!("Retired '{name}' ({reason}). It's out of every tool list; the files are kept in ~/.viv/tools/.retired/."),
                is_error: false,
                details: None,
            },
            Err(e) => err(e),
        }
    }
}

/// The review sheet: every tool with how much it's used.
pub fn describe_all() -> String {
    let defs = load_custom_tools();
    if defs.is_empty() {
        return "No custom tools yet. Create one with create_tool when a task needs something the built-in tools don't do.".into();
    }
    let mut out = format!("{} custom tools (review past {SOFT_LIMIT}):\n", defs.len());
    for d in defs {
        let last = d.last_used.as_deref().and_then(|s| s.get(..10)).unwrap_or("never");
        let made = d.created_at.get(..10).unwrap_or("?");
        out.push_str(&format!("\n- {} — {}\n  uses: {} · last used: {last} · created: {made}", d.name, d.description, d.uses));
    }
    out
}

pub struct ListCustomToolsTool;

#[async_trait]
impl Tool for ListCustomToolsTool {
    fn name(&self) -> &str {
        "list_custom_tools"
    }

    async fn execute(&self, _args: &serde_json::Value, _cwd: &str) -> ToolResult {
        ToolResult { content: describe_all(), is_error: false, details: None }
    }
}

// ── Settings ────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn cmd_list_custom_tools() -> Result<Vec<CustomToolDef>, String> {
    Ok(load_custom_tools())
}

#[tauri::command]
pub async fn cmd_delete_custom_tool(name: String) -> Result<(), String> {
    retire(&name)
}

/// The tool's source, for viewing in Settings.
#[tauri::command]
pub async fn cmd_custom_tool_source(name: String) -> Result<String, String> {
    check_name_exists(&name)?;
    std::fs::read_to_string(tools_dir()?.join(&name).join("tool.py")).map_err(|e| e.to_string())
}

fn check_name_exists(name: &str) -> Result<(), String> {
    if name.contains(['/', '\\', '.']) || read_def(&tools_dir()?.join(name)).is_none() {
        return Err(format!("There's no custom tool named '{name}'."));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_checked() {
        assert!(check_name("csv_to_parquet").is_ok());
        assert!(check_name("Bad-Name").is_err());
        assert!(check_name("ab").is_err());
        assert!(check_name("read").is_err());
        assert!(check_name("create_tool").is_err());
        assert!(check_name("mcp__x__y").is_err());
    }

    #[tokio::test]
    async fn runs_a_tool_and_reports_its_errors() {
        if super::super::checkers::python().is_none() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("viv-ct-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("tool.py"), "def run(a, b=2):
    return {'sum': a + b, 'word': 'héllo'}
").unwrap();
        let ok = run_python(&dir, &serde_json::json!({"a": 40}), ".", Duration::from_secs(60)).await;
        assert!(!ok.is_error, "{}", ok.content);
        assert!(ok.content.contains("\"sum\": 42") && ok.content.contains("héllo"), "{}", ok.content);

        std::fs::write(dir.join("tool.py"), "def run():
    raise ValueError('nope')
").unwrap();
        let bad = run_python(&dir, &serde_json::json!({}), ".", Duration::from_secs(60)).await;
        assert!(bad.is_error && bad.content.contains("ValueError: nope"), "{}", bad.content);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn output_is_clipped_on_a_char_boundary() {
        let s = "é".repeat(MAX_OUTPUT_CHARS);
        let out = clip(s);
        assert!(out.ends_with("(output truncated)"));
    }
}

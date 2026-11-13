//! Which language server runs for a file, where it is rooted, and what it is told at start.
//!
//! Three sources, in this order: the user's own `lsp.json` in their Vivianne folder, the project's `lsp.json`, then the built-in table
//! (`server_candidates`). **A project's file is ignored until the user has trusted that project** (the same trust the project's mods use): a
//! cloned repository must not be able to name a command Vivianne runs on its own. What is ignored is still reported, with the full command, so
//! the user can see what trusting would allow.
//!
//! ```json
//! { "servers": { "python": { "command": "pyright-langserver", "args": ["--stdio"], "rootMarkers": ["pyrightconfig.json"],
//!                            "initOptions": {}, "settings": { "python": { "analysis": { "typeCheckingMode": "strict" } } } } } }
//! ```

use serde_json::Value;
use std::path::Path;

/// Where a server definition came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Builtin,
    User,
    Project,
}

/// One server to try for a language.
#[derive(Debug, Clone, PartialEq)]
pub struct ServerDef {
    pub command: String,
    pub args: Vec<String>,
    pub root_markers: Vec<String>,
    pub init_options: Value,
    pub settings: Value,
    pub source: Source,
}

impl ServerDef {
    /// The command line as it would be typed.
    pub fn line(&self) -> String {
        std::iter::once(self.command.as_str()).chain(self.args.iter().map(|a| a.as_str())).collect::<Vec<_>>().join(" ")
    }
}

/// Files that mark the root of a project for a language: the server is started there, not at the workspace folder, so a monorepo's packages
/// each get the configuration that belongs to them.
pub fn root_markers(language_id: &str) -> &'static [&'static str] {
    match language_id {
        "typescript" | "typescriptreact" | "javascript" | "javascriptreact" => &["tsconfig.json", "jsconfig.json", "package.json"],
        "python" => &["pyproject.toml", "pyrightconfig.json", "setup.py", "setup.cfg", "requirements.txt", "Pipfile"],
        "rust" => &["Cargo.toml"],
        "go" => &["go.work", "go.mod"],
        "c" | "cpp" => &["compile_commands.json", "compile_flags.txt", ".clangd", "CMakeLists.txt"],
        "csharp" => &["global.json", "Directory.Build.props"],
        "java" => &["pom.xml", "build.gradle", "build.gradle.kts", "settings.gradle"],
        "kotlin" | "scala" => &["build.gradle.kts", "build.gradle", "build.sbt", "pom.xml"],
        "ruby" => &["Gemfile", ".ruby-version"],
        "php" => &["composer.json"],
        "dart" => &["pubspec.yaml"],
        "elixir" => &["mix.exs"],
        "haskell" => &["stack.yaml", "cabal.project", "hie.yaml"],
        "zig" => &["build.zig"],
        "swift" => &["Package.swift"],
        "vue" | "svelte" => &["package.json"],
        "terraform" => &[".terraform", "main.tf"],
        _ => &[],
    }
}

/// The folder a server for `file` is rooted at: the nearest ancestor (up to the workspace) holding one of the language's marker files, else the
/// workspace itself. A file outside the workspace is served from the workspace.
pub fn find_root(file: &Path, workspace: &str, markers: &[String]) -> String {
    let ws = super::uri::path_key(workspace);
    let inside = |d: &Path| {
        let k = super::uri::path_key(&d.to_string_lossy());
        k == ws || k.starts_with(&format!("{}/", ws.trim_end_matches('/')))
    };
    let mut dir = file.parent();
    while let Some(d) = dir {
        if !inside(d) {
            break;
        }
        if markers.iter().any(|m| d.join(m).exists()) {
            return d.to_string_lossy().into_owned();
        }
        if super::uri::path_key(&d.to_string_lossy()) == ws {
            break;
        }
        dir = d.parent();
    }
    workspace.to_string()
}

/// What a language's config entry says, or why it can't be used.
fn parse_entry(entry: &Value, source: Source) -> Option<ServerDef> {
    let command = entry.get("command")?.as_str()?.trim();
    if command.is_empty() {
        return None;
    }
    let strings = |k: &str| -> Vec<String> { entry.get(k).and_then(|a| a.as_array()).map(|a| a.iter().filter_map(|s| s.as_str().map(String::from)).collect()).unwrap_or_default() };
    Some(ServerDef {
        command: command.to_string(),
        args: strings("args"),
        root_markers: strings("rootMarkers"),
        // `initializationOptions` is what the protocol calls it, and what other editors' files use.
        init_options: entry.get("initOptions").or_else(|| entry.get("initializationOptions")).cloned().unwrap_or(Value::Null),
        settings: entry.get("settings").cloned().unwrap_or(Value::Null),
        source,
    })
}

/// The definitions a config file gives `language_id`. A file that isn't valid JSON gives none (and the caller says so).
pub fn from_config(text: &str, language_id: &str, source: Source) -> Result<Vec<ServerDef>, String> {
    let doc: Value = serde_json::from_str(text).map_err(|e| format!("not valid JSON: {e}"))?;
    // `{ "servers": { "python": ... } }`, or the same map at the top level.
    let entry = if doc.get("servers").is_some() { doc.pointer(&format!("/servers/{language_id}")) } else { doc.get(language_id) };
    let Some(entry) = entry else { return Ok(Vec::new()) };
    let entries: Vec<&Value> = match entry {
        Value::Array(a) => a.iter().collect(),
        other => vec![other],
    };
    Ok(entries.into_iter().filter_map(|e| parse_entry(e, source)).collect())
}

/// Where a project keeps programs of its own, tried before PATH — but only in a trusted project: a repository's `node_modules/.bin` is code
/// that came with the repository.
pub const LOCAL_BIN_DIRS: &[&str] = &["node_modules/.bin", ".venv/bin", ".venv/Scripts", "venv/bin", "venv/Scripts", ".env/bin", "vendor/bundle/bin"];

/// `command` as it should be started: a project-local program when the project is trusted and has one, else what PATH finds (`on_path`).
pub fn resolve_command_with(command: &str, root: &Path, trusted: bool, exists: &dyn Fn(&Path) -> bool, on_path: &dyn Fn(&str) -> Option<String>) -> Option<String> {
    if trusted && !command.contains(['/', '\\']) {
        for dir in LOCAL_BIN_DIRS {
            for ext in ["", ".cmd", ".exe", ".bat"] {
                let candidate = root.join(dir).join(format!("{command}{ext}"));
                if exists(&candidate) {
                    return Some(candidate.to_string_lossy().into_owned());
                }
            }
        }
    }
    on_path(command)
}

/// What to try for a language, and what was left out.
#[derive(Debug, Default)]
pub struct Candidates {
    pub defs: Vec<ServerDef>,
    /// Command lines a project defines that are not used because the project is not trusted.
    pub ignored: Vec<String>,
    /// A config file that could not be read as JSON.
    pub problems: Vec<String>,
}

/// User config, then project config (only when `project_trusted`), then the built-in table.
pub fn candidates_with(language_id: &str, user_cfg: Option<&str>, project_cfg: Option<&str>, project_trusted: bool) -> Candidates {
    let mut out = Candidates::default();
    if let Some(text) = user_cfg {
        match from_config(text, language_id, Source::User) {
            Ok(d) => out.defs.extend(d),
            Err(e) => out.problems.push(format!("your lsp.json is {e}")),
        }
    }
    if let Some(text) = project_cfg {
        match from_config(text, language_id, Source::Project) {
            Ok(d) if project_trusted => out.defs.extend(d),
            Ok(d) => out.ignored.extend(d.iter().map(|s| s.line())),
            Err(e) => out.problems.push(format!("the project's lsp.json is {e}")),
        }
    }
    let markers: Vec<String> = root_markers(language_id).iter().map(|m| m.to_string()).collect();
    for (cmd, args) in super::server_candidates(language_id) {
        out.defs.push(ServerDef { command: cmd.to_string(), args: args.iter().map(|a| a.to_string()).collect(), root_markers: markers.clone(), init_options: Value::Null, settings: Value::Null, source: Source::Builtin });
    }
    // A definition that names no markers of its own takes the language's.
    for d in out.defs.iter_mut().filter(|d| d.root_markers.is_empty()) {
        d.root_markers = markers.clone();
    }
    out
}

/// The same, reading the files: the user's from their Vivianne folder, the project's from `<workspace>/.viv/lsp.json`.
pub fn candidates(language_id: &str, workspace: &str) -> Candidates {
    let user = std::fs::read_to_string(crate::agent::system::paths::viv_home_path(&["lsp.json"])).ok();
    let project = std::fs::read_to_string(Path::new(workspace).join(".viv").join("lsp.json")).ok();
    let trusted = project.is_some() && crate::mods::project_trusted(workspace);
    candidates_with(language_id, user.as_deref(), project.as_deref(), trusted)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const CFG: &str = r#"{ "servers": { "python": { "command": "./evil/ls", "args": ["--stdio"], "settings": { "a": 1 } }, "rust": [{ "command": "ra1" }, { "command": "ra2" }] } }"#;

    #[test]
    fn a_projects_servers_are_ignored_until_the_project_is_trusted() {
        let untrusted = candidates_with("python", None, Some(CFG), false);
        assert!(untrusted.defs.iter().all(|d| d.source == Source::Builtin), "{:?}", untrusted.defs);
        assert_eq!(untrusted.ignored, vec!["./evil/ls --stdio".to_string()], "what trusting would allow is shown in full");
        let trusted = candidates_with("python", None, Some(CFG), true);
        assert_eq!(trusted.defs[0].source, Source::Project);
        assert_eq!(trusted.defs[0].line(), "./evil/ls --stdio");
        assert_eq!(trusted.defs[0].settings, json!({ "a": 1 }));
        assert!(trusted.ignored.is_empty());
        // Built-ins still follow, as the fallback.
        assert!(trusted.defs.iter().any(|d| d.source == Source::Builtin));
    }

    #[test]
    fn the_users_own_file_always_counts_and_comes_first() {
        let c = candidates_with("rust", Some(CFG), Some(r#"{ "servers": { "rust": { "command": "proj" } } }"#), true);
        let order: Vec<&str> = c.defs.iter().map(|d| d.command.as_str()).collect();
        assert_eq!(&order[..3], &["ra1", "ra2", "proj"], "a language can list several, tried in order");
    }

    #[test]
    fn a_broken_file_is_reported_and_does_not_stop_the_built_ins() {
        let c = candidates_with("rust", Some("{ nope"), None, false);
        assert!(c.problems[0].contains("lsp.json"), "{:?}", c.problems);
        assert!(c.defs.iter().any(|d| d.command == "rust-analyzer"));
    }

    #[test]
    fn a_definition_takes_the_languages_root_markers_unless_it_names_its_own() {
        let c = candidates_with("python", Some(r#"{ "servers": { "python": { "command": "x", "rootMarkers": ["mine.cfg"] } } }"#), None, false);
        assert_eq!(c.defs[0].root_markers, vec!["mine.cfg".to_string()]);
        assert!(c.defs.last().unwrap().root_markers.contains(&"pyproject.toml".to_string()));
    }

    #[test]
    fn a_server_is_rooted_at_the_nearest_marker_inside_the_workspace() {
        let ws = std::env::temp_dir().join(format!("viv-root-{}", uuid::Uuid::new_v4()));
        let pkg = ws.join("packages/app/src");
        std::fs::create_dir_all(&pkg).unwrap();
        std::fs::write(ws.join("Cargo.toml"), "").unwrap();
        std::fs::write(ws.join("packages/app/Cargo.toml"), "").unwrap();
        let markers = vec!["Cargo.toml".to_string()];
        let file = pkg.join("main.rs");
        assert_eq!(find_root(&file, &ws.to_string_lossy(), &markers), ws.join("packages/app").to_string_lossy());
        // No marker below the workspace: the workspace. A file elsewhere: the workspace.
        assert_eq!(find_root(&file, &ws.to_string_lossy(), &["nothing".to_string()]), ws.to_string_lossy());
        assert_eq!(find_root(Path::new("/elsewhere/x.rs"), &ws.to_string_lossy(), &markers), ws.to_string_lossy());
        std::fs::remove_dir_all(&ws).ok();
    }

    #[test]
    fn project_local_programs_count_only_in_a_trusted_project() {
        let root = Path::new("/w");
        let exists = |p: &Path| p == Path::new("/w/node_modules/.bin/pyright-langserver");
        let path = |c: &str| Some(format!("/usr/bin/{c}"));
        assert_eq!(resolve_command_with("pyright-langserver", root, true, &exists, &path).map(|p| p.replace('\\', "/")).as_deref(), Some("/w/node_modules/.bin/pyright-langserver"));
        assert_eq!(resolve_command_with("pyright-langserver", root, false, &exists, &path).as_deref(), Some("/usr/bin/pyright-langserver"), "untrusted: PATH only");
        assert_eq!(resolve_command_with("./x/ls", root, true, &exists, &path).as_deref(), Some("/usr/bin/./x/ls"), "a path is never looked up in the project's bin dirs");
        assert_eq!(resolve_command_with("gopls", root, true, &|_| false, &|_| None), None);
    }

    #[test]
    fn a_flat_map_and_the_protocols_option_name_are_accepted() {
        let flat = r#"{ "python": { "command": "x", "initializationOptions": { "a": 1 } } }"#;
        let d = from_config(flat, "python", Source::User).unwrap();
        assert_eq!((d[0].command.as_str(), d[0].init_options.clone()), ("x", json!({ "a": 1 })));
        assert!(from_config(flat, "rust", Source::User).unwrap().is_empty());
    }
}

//! "Open in…": the editors installed on this machine, and opening a file or folder in one of them.
//!
//! Detection is a look along PATH for each editor's command line launcher, so only apps the user can actually start from a terminal are
//! offered. Opening runs the launcher with plain arguments (never through a shell), at a line when one is given.

use std::path::Path;

use serde::Serialize;

struct Editor {
    id: &'static str,
    label: &'static str,
    /// Command names to look for, first found wins.
    commands: &'static [&'static str],
    /// How a line number is passed: `file:line` for most editors.
    goto: Goto,
}

#[derive(Clone, Copy)]
enum Goto {
    /// `--goto file:line`
    Flag,
    /// `file:line`
    Suffix,
    /// The editor takes a plain path only.
    None,
}

const EDITORS: &[Editor] = &[
    Editor { id: "vscode", label: "Visual Studio Code", commands: &["code"], goto: Goto::Flag },
    Editor { id: "cursor", label: "Cursor", commands: &["cursor"], goto: Goto::Flag },
    Editor { id: "windsurf", label: "Windsurf", commands: &["windsurf"], goto: Goto::Flag },
    Editor { id: "zed", label: "Zed", commands: &["zed"], goto: Goto::Suffix },
    Editor { id: "sublime", label: "Sublime Text", commands: &["subl"], goto: Goto::Suffix },
    Editor { id: "notepadpp", label: "Notepad++", commands: &["notepad++"], goto: Goto::None },
];

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct OpenApp {
    pub id: String,
    pub label: String,
}

/// What to run for an editor: the launcher's name and its arguments.
fn plan(editor: &Editor, path: &str, line: Option<u32>) -> Vec<String> {
    match (editor.goto, line) {
        (Goto::Flag, Some(l)) => vec!["--goto".into(), format!("{path}:{l}")],
        (Goto::Suffix, Some(l)) => vec![format!("{path}:{l}")],
        _ => vec![path.to_string()],
    }
}

fn editor(id: &str) -> Option<&'static Editor> {
    EDITORS.iter().find(|e| e.id == id)
}

fn launcher(e: &Editor) -> Option<std::path::PathBuf> {
    e.commands.iter().find_map(|c| which::which(c).ok())
}

/// The editors found on this machine, plus the system's own ways of opening a file.
#[tauri::command]
pub async fn cmd_list_open_apps() -> Result<Vec<OpenApp>, String> {
    let mut apps: Vec<OpenApp> = EDITORS.iter().filter(|e| launcher(e).is_some()).map(|e| OpenApp { id: e.id.into(), label: e.label.into() }).collect();
    apps.push(OpenApp { id: "default".into(), label: "Default app".into() });
    apps.push(OpenApp { id: "reveal".into(), label: "Show in folder".into() });
    Ok(apps)
}

/// Open `path` in the app named by `app` (an id from `cmd_list_open_apps`), at `line` when the app can.
#[tauri::command]
pub async fn cmd_open_in_app(path: String, app: String, line: Option<u32>) -> Result<(), String> {
    let p = Path::new(&path);
    if !p.is_absolute() || !p.exists() {
        return Err(format!("{path} isn't a file or folder on this machine."));
    }
    match app.as_str() {
        "default" => crate::agent::sessions::projects::cmd_open_path(path).await,
        "reveal" => reveal(p),
        id => {
            let e = editor(id).ok_or_else(|| format!("Unknown app '{id}'."))?;
            let exe = launcher(e).ok_or_else(|| format!("{} isn't installed (its command line launcher wasn't found).", e.label))?;
            let mut cmd = std::process::Command::new(exe);
            cmd.args(plan(e, &path, line));
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                cmd.creation_flags(0x0800_0000);
            }
            cmd.spawn().map(|_| ()).map_err(|er| format!("Couldn't start {}: {er}", e.label))
        }
    }
}

fn reveal(p: &Path) -> Result<(), String> {
    #[cfg(windows)]
    let spawned = std::process::Command::new("explorer").arg(format!("/select,{}", p.display())).spawn();
    #[cfg(target_os = "macos")]
    let spawned = std::process::Command::new("open").arg("-R").arg(p).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let spawned = std::process::Command::new("xdg-open").arg(if p.is_dir() { p } else { p.parent().unwrap_or(p) }).spawn();
    spawned.map(|_| ()).map_err(|e| format!("couldn't show {} in its folder: {e}", p.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_is_passed_the_way_each_editor_wants_it() {
        assert_eq!(plan(editor("vscode").unwrap(), "/w/a.rs", Some(12)), vec!["--goto", "/w/a.rs:12"]);
        assert_eq!(plan(editor("zed").unwrap(), "/w/a.rs", Some(12)), vec!["/w/a.rs:12"]);
        assert_eq!(plan(editor("notepadpp").unwrap(), "/w/a.rs", Some(12)), vec!["/w/a.rs"]);
        assert_eq!(plan(editor("vscode").unwrap(), "/w/a.rs", None), vec!["/w/a.rs"]);
    }

    #[test]
    fn ids_are_unique_and_unknown_apps_are_refused() {
        let mut ids: Vec<&str> = EDITORS.iter().map(|e| e.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), EDITORS.len());
        assert!(editor("emacs-from-the-future").is_none());
        let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
        assert!(rt.block_on(cmd_open_in_app("relative/path".into(), "vscode".into(), None)).is_err());
        let here = std::env::current_dir().unwrap().display().to_string();
        assert!(rt.block_on(cmd_open_in_app(here, "nope".into(), None)).unwrap_err().contains("Unknown app"));
    }

    #[test]
    fn the_list_always_offers_the_system_choices() {
        let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
        let apps = rt.block_on(cmd_list_open_apps()).unwrap();
        assert!(apps.iter().any(|a| a.id == "default") && apps.iter().any(|a| a.id == "reveal"));
    }
}

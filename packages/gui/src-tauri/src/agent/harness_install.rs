//! Installing a coding agent where Vivianne owns it.
//!
//! `npx -y <package>` fetches an agent on its first run and says nothing while
//! it does. The Claude Code agent bundles the Claude Code binary, so that first
//! run is a very large download and looks exactly like a hang: nothing on
//! stdout, two npm warnings on stderr, and a turn that waits minutes. Here the
//! package is installed once into `~/.viv/agents/<id>/`, with its progress
//! shown, a receipt records where its program is, and from then on the agent
//! starts as `node <script>` with no `npx` and no network.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// What an install left behind.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Receipt {
    /// `name@version`, as the registry named it.
    pub package: String,
    /// The program to run with `node`, absolute.
    pub script: String,
}

pub fn agent_dir(id: &str) -> PathBuf {
    let safe: String = id.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
    crate::agent::system::paths::viv_home_path(&["agents", &safe])
}

/// `@scope/name@1.2.3` as (`@scope/name`, Some(`1.2.3`)); a name with no version has none.
pub fn split_package(spec: &str) -> (String, Option<String>) {
    let at = spec.char_indices().skip(1).find(|(_, c)| *c == '@').map(|(i, _)| i);
    match at {
        Some(i) => (spec[..i].to_string(), Some(spec[i + 1..].to_string())),
        None => (spec.to_string(), None),
    }
}

/// The script a package's `bin` names: its string form, or the entry named
/// like the package (without its scope), or the first one.
pub fn bin_script(package_json: &Value, package_name: &str) -> Option<String> {
    let bin = package_json.get("bin")?;
    if let Some(s) = bin.as_str() {
        return Some(s.to_string());
    }
    let map = bin.as_object()?;
    let short = package_name.rsplit('/').next().unwrap_or(package_name);
    map.get(short).or_else(|| map.values().next()).and_then(|v| v.as_str()).map(String::from)
}

/// The receipt for `id`, if it matches the package the registry lists now
/// (a newer registry version needs a fresh install) and its program is still there.
pub fn receipt_for(id: &str, package: &str) -> Option<Receipt> {
    let text = std::fs::read_to_string(agent_dir(id).join("receipt.json")).ok()?;
    let r: Receipt = serde_json::from_str(&text).ok()?;
    (r.package == package && Path::new(&r.script).is_file()).then_some(r)
}

pub fn write_receipt(id: &str, receipt: &Receipt) -> Result<(), String> {
    let dir = agent_dir(id);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let json = serde_json::to_string_pretty(receipt).map_err(|e| e.to_string())?;
    std::fs::write(dir.join("receipt.json"), json).map_err(|e| e.to_string())
}

/// The program of an installed package: `node_modules/<name>/<bin script>` in `dir`.
pub fn installed_script(dir: &Path, package: &str) -> Result<PathBuf, String> {
    let (name, _) = split_package(package);
    let root = dir.join("node_modules").join(name.replace('/', std::path::MAIN_SEPARATOR_STR));
    let text = std::fs::read_to_string(root.join("package.json")).map_err(|e| format!("The package didn't install: no package.json in {} ({e})", root.display()))?;
    let json: Value = serde_json::from_str(&text).map_err(|e| format!("The package's package.json is invalid: {e}"))?;
    let script = bin_script(&json, &name).ok_or("The package doesn't say which program to run (no `bin`).")?;
    let path = root.join(script.trim_start_matches("./"));
    path.is_file().then_some(path).ok_or_else(|| "The package's program is missing after the install.".to_string())
}

/// What an npm progress line says, in words, or nothing for a line that isn't progress.
/// With `--loglevel http` npm prints one line per request, e.g.
/// `npm http fetch GET 200 https://registry.npmjs.org/x/-/x-1.2.3.tgz 1234ms (cache miss)`.
pub fn progress_line(line: &str) -> Option<String> {
    let l = line.trim();
    if l.contains("npm warn") || l.is_empty() {
        return None;
    }
    if let Some(rest) = l.strip_prefix("npm http fetch ") {
        let mut parts = rest.split_whitespace();
        let (_method, status, url) = (parts.next()?, parts.next()?, parts.next()?);
        let file = url.rsplit('/').next().unwrap_or(url);
        return Some(if file.ends_with(".tgz") { format!("Downloaded {file}") } else { format!("Looked up {} ({status})", file.trim_end_matches(".json")) });
    }
    if let Some(rest) = l.strip_prefix("npm http cache ") {
        // `npm http cache <name>@<url> 0ms (cache hit)`
        let url = rest.split_whitespace().next().unwrap_or("");
        let file = url.rsplit('/').next().unwrap_or(url);
        return Some(format!("Using the copy npm already had of {}", file.trim_end_matches(".tgz")));
    }
    if l.starts_with("npm error") || l.starts_with("npm ERR") {
        return Some(l.trim_start_matches("npm error").trim_start_matches("npm ERR!").trim().to_string());
    }
    None
}

/// Total size of a folder in bytes, counting at most `limit` entries (a cheap size for a progress line).
pub fn dir_bytes(dir: &Path, limit: usize) -> u64 {
    let mut total = 0;
    let mut seen = 0;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            seen += 1;
            if seen > limit {
                return total;
            }
            let Ok(meta) = e.metadata() else { continue };
            if meta.is_dir() {
                stack.push(e.path());
            } else {
                total += meta.len();
            }
        }
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_package_spec_splits_into_name_and_version() {
        assert_eq!(split_package("@agentclientprotocol/claude-agent-acp@0.100.0"), ("@agentclientprotocol/claude-agent-acp".into(), Some("0.100.0".into())));
        assert_eq!(split_package("cline@1.2.3"), ("cline".into(), Some("1.2.3".into())));
        assert_eq!(split_package("@scope/pkg"), ("@scope/pkg".into(), None));
        assert_eq!(split_package("pkg"), ("pkg".into(), None));
    }

    #[test]
    fn the_program_is_found_in_either_form_of_bin() {
        assert_eq!(bin_script(&json!({ "bin": "dist/index.js" }), "x").as_deref(), Some("dist/index.js"));
        assert_eq!(bin_script(&json!({ "bin": { "claude-agent-acp": "dist/a.js", "other": "dist/b.js" } }), "@agentclientprotocol/claude-agent-acp").as_deref(), Some("dist/a.js"));
        assert_eq!(bin_script(&json!({ "bin": { "tool": "t.js" } }), "different-name").as_deref(), Some("t.js"), "no name match: the first");
        assert_eq!(bin_script(&json!({}), "x"), None);
        assert_eq!(bin_script(&json!({ "bin": 3 }), "x"), None);
    }

    #[test]
    fn npm_lines_become_progress_and_noise_is_dropped() {
        assert_eq!(progress_line("npm http fetch GET 200 https://registry.npmjs.org/@anthropic-ai/claude-agent-sdk-win32-x64/-/claude-agent-sdk-win32-x64-0.3.284.tgz 41233ms (cache miss)").as_deref(), Some("Downloaded claude-agent-sdk-win32-x64-0.3.284.tgz"));
        assert_eq!(progress_line("npm http fetch GET 200 https://registry.npmjs.org/zod 120ms").as_deref(), Some("Looked up zod (200)"));
        assert_eq!(progress_line("npm http cache content-type@https://registry.npmjs.org/content-type/-/content-type-2.1.0.tgz 0ms (cache hit)").as_deref(), Some("Using the copy npm already had of content-type-2.1.0"));
        assert_eq!(progress_line("npm warn Unknown env config \"min-release-age\"."), None);
        assert_eq!(progress_line("npm error code E404").as_deref(), Some("code E404"));
        assert_eq!(progress_line("   "), None);
    }

    #[test]
    fn an_installed_package_is_found_through_its_package_json() {
        let d = std::env::temp_dir().join(format!("viv-agent-{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let pkg = d.join("node_modules").join("@scope").join("agent");
        std::fs::create_dir_all(pkg.join("dist")).unwrap();
        std::fs::write(pkg.join("package.json"), r#"{"name":"@scope/agent","bin":{"agent":"./dist/run.js"}}"#).unwrap();
        std::fs::write(pkg.join("dist").join("run.js"), "// agent").unwrap();
        let script = installed_script(&d, "@scope/agent@1.0.0").unwrap();
        assert!(script.ends_with("dist/run.js") || script.ends_with("dist\\run.js"), "{}", script.display());
        assert!(dir_bytes(&d, 1000) >= 8);
        // The program missing is an error that says so.
        std::fs::remove_file(pkg.join("dist").join("run.js")).unwrap();
        assert!(installed_script(&d, "@scope/agent@1.0.0").unwrap_err().contains("missing"));
        assert!(installed_script(&d.join("nowhere"), "@scope/agent").unwrap_err().contains("didn't install"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_receipt_counts_only_for_the_package_listed_now_and_while_its_program_exists() {
        let id = format!("test-receipt-{}", std::process::id());
        let dir = agent_dir(&id);
        std::fs::create_dir_all(&dir).unwrap();
        let script = dir.join("run.js");
        std::fs::write(&script, "// x").unwrap();
        write_receipt(&id, &Receipt { package: "a@1.0.0".into(), script: script.to_string_lossy().to_string() }).unwrap();
        assert!(receipt_for(&id, "a@1.0.0").is_some());
        assert!(receipt_for(&id, "a@1.1.0").is_none(), "a newer registry version needs a fresh install");
        std::fs::remove_file(&script).unwrap();
        assert!(receipt_for(&id, "a@1.0.0").is_none(), "the program is gone");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

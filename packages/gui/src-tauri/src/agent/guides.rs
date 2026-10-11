//! Vivianne's guides: the design taste behind her documents and the way she
//! gets to know and carry a project, kept as plain Markdown in a public
//! repository (`packages/vivianne-guides`, published to GitHub).
//!
//! The `guide` tool reads one guide at a time, so a task pays only for what
//! it needs. It reads from a local clone of the repository (the user can
//! point Settings at their own fork: their brand, their way of working),
//! refreshed in the background at most once a day, and falls back to the copy
//! built into the app when there is no clone yet or no network.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

pub const DEFAULT_REPO: &str = "https://github.com/v3dqnt/vivianne-guides";
const REFRESH: Duration = Duration::from_secs(24 * 3600);

/// The copy built into the app.
const EMBEDDED: &[(&str, &str)] = &[
    ("README.md", include_str!("../../../../vivianne-guides/README.md")),
    ("design/checklist.md", include_str!("../../../../vivianne-guides/design/checklist.md")),
    ("design/color.md", include_str!("../../../../vivianne-guides/design/color.md")),
    ("design/data.md", include_str!("../../../../vivianne-guides/design/data.md")),
    ("design/formats/docx.md", include_str!("../../../../vivianne-guides/design/formats/docx.md")),
    ("design/formats/graphics.md", include_str!("../../../../vivianne-guides/design/formats/graphics.md")),
    ("design/formats/pdf.md", include_str!("../../../../vivianne-guides/design/formats/pdf.md")),
    ("design/formats/slides.md", include_str!("../../../../vivianne-guides/design/formats/slides.md")),
    ("design/formats/xlsx.md", include_str!("../../../../vivianne-guides/design/formats/xlsx.md")),
    ("design/index.md", include_str!("../../../../vivianne-guides/design/index.md")),
    ("design/layout.md", include_str!("../../../../vivianne-guides/design/layout.md")),
    ("design/principles.md", include_str!("../../../../vivianne-guides/design/principles.md")),
    ("design/templates/cv.html", include_str!("../../../../vivianne-guides/design/templates/cv.html")),
    ("design/templates/invoice.html", include_str!("../../../../vivianne-guides/design/templates/invoice.html")),
    ("design/templates/letter.html", include_str!("../../../../vivianne-guides/design/templates/letter.html")),
    ("design/templates/poster.html", include_str!("../../../../vivianne-guides/design/templates/poster.html")),
    ("design/templates/social.html", include_str!("../../../../vivianne-guides/design/templates/social.html")),
    ("design/typography.md", include_str!("../../../../vivianne-guides/design/typography.md")),
    ("design/writing.md", include_str!("../../../../vivianne-guides/design/writing.md")),
    ("science/biology.md", include_str!("../../../../vivianne-guides/science/biology.md")),
    ("science/chemistry.md", include_str!("../../../../vivianne-guides/science/chemistry.md")),
    ("science/index.md", include_str!("../../../../vivianne-guides/science/index.md")),
    ("science/physics.md", include_str!("../../../../vivianne-guides/science/physics.md")),
    ("mods/index.md", include_str!("../../../../vivianne-guides/mods/index.md")),
    ("projects/existing-project.md", include_str!("../../../../vivianne-guides/projects/existing-project.md")),
    ("projects/index.md", include_str!("../../../../vivianne-guides/projects/index.md")),
    ("projects/knowledge.md", include_str!("../../../../vivianne-guides/projects/knowledge.md")),
    ("projects/new-project.md", include_str!("../../../../vivianne-guides/projects/new-project.md")),
    ("projects/roadmap.md", include_str!("../../../../vivianne-guides/projects/roadmap.md")),
    ("projects/team.md", include_str!("../../../../vivianne-guides/projects/team.md")),
    ("projects/working.md", include_str!("../../../../vivianne-guides/projects/working.md")),
];

fn clone_dir() -> Option<PathBuf> {
    crate::agent::paths::viv_home().ok().map(|h| h.join("guides"))
}

async fn repo_url() -> String {
    crate::agent::tool_exec::read_settings_json()
        .await
        .get("guidesRepo")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| s.starts_with("https://") || s.starts_with("git@"))
        .unwrap_or(DEFAULT_REPO)
        .to_string()
}

fn git(args: &[&str], cwd: Option<&Path>) -> bool {
    let mut cmd = std::process::Command::new("git");
    cmd.args(args).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    if let Some(d) = cwd {
        cmd.current_dir(d);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    cmd.status().map(|s| s.success()).unwrap_or(false)
}

/// Clone or refresh the local copy in the background; never blocks a read.
fn sync(url: String) {
    static RUNNING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    let Some(dir) = clone_dir() else { return };
    let marker = dir.join(".git").join("FETCH_HEAD");
    let fresh = std::fs::metadata(&marker).or_else(|_| std::fs::metadata(dir.join(".git"))).and_then(|m| m.modified()).ok().and_then(|t| SystemTime::now().duration_since(t).ok()).is_some_and(|age| age < REFRESH);
    if fresh || RUNNING.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    std::thread::spawn(move || {
        let origin_ok = dir.join(".git").exists()
            && std::process::Command::new("git").args(["config", "--get", "remote.origin.url"]).current_dir(&dir).output().ok().map(|o| String::from_utf8_lossy(&o.stdout).trim() == url).unwrap_or(false);
        if origin_ok {
            git(&["pull", "--ff-only", "--depth", "1"], Some(&dir));
        } else {
            // A different repository was chosen in Settings: start over.
            let _ = std::fs::remove_dir_all(&dir);
            git(&["clone", "--depth", "1", &url, &dir.display().to_string()], None);
        }
        RUNNING.store(false, std::sync::atomic::Ordering::SeqCst);
    });
}

/// "design/typography", "design/typography.md", "design" (its index),
/// "" (the overview), "design/templates/cv" (a template).
fn candidates(path: &str) -> Vec<String> {
    let p = path.trim().trim_matches('/').replace('\\', "/");
    if p.is_empty() {
        return vec!["README.md".into()];
    }
    if p.ends_with(".md") || p.ends_with(".html") {
        return vec![p];
    }
    vec![format!("{p}.md"), format!("{p}/index.md"), format!("{p}.html")]
}

fn read_local(dir: &Path, rel: &str) -> Option<String> {
    if rel.split('/').any(|seg| seg == ".." || seg.is_empty()) {
        return None;
    }
    std::fs::read_to_string(dir.join(rel)).ok()
}

fn list() -> String {
    let mut out = String::from("Available guides (pass one as `path`):\n");
    for (p, _) in EMBEDDED {
        let shown = p.trim_end_matches(".md").trim_end_matches("/index");
        out.push_str(&format!("- {shown}\n"));
    }
    out
}

/// One guide's text, from the local clone if there is one, else built in.
pub async fn read(path: &str) -> Result<String, String> {
    let url = repo_url().await;
    sync(url.clone());
    let dir = clone_dir().filter(|d| d.join(".git").exists());
    for rel in candidates(path) {
        if let Some(text) = dir.as_deref().and_then(|d| read_local(d, &rel)) {
            return Ok(text);
        }
        if url == DEFAULT_REPO || dir.is_none() {
            if let Some((_, text)) = EMBEDDED.iter().find(|(p, _)| *p == rel) {
                return Ok((*text).to_string());
            }
        }
    }
    Err(format!("No guide called '{path}'.\n{}", list()))
}

pub fn schema() -> serde_json::Value {
    serde_json::json!({
        "name": "guide",
        "description": "Read Vivianne's guides: how documents should look, and how to get to know and carry a project. Read the relevant ones BEFORE the work, not after.\n- Making any document, deck, spreadsheet or graphic: read `design/principles`, then the format guide (`design/formats/pdf`, `design/formats/slides`, `design/formats/docx`, `design/formats/xlsx`, `design/formats/graphics`) and anything else it points to (`design/typography`, `design/data`, `design/layout`…). Templates: `design/templates/cv`, `invoice`, `letter`, `poster`, `social`.\n- Starting a new project: `projects/new-project`. Joining or first working on an existing one: `projects/existing-project`. The project knowledge files: `projects/knowledge`. Day-to-day: `projects/working`, `projects/roadmap`, `projects/team`.\n- Putting chemistry, biology, physics or maths in a reply as drawings: `science/chemistry` (structures, reactions, mechanisms, 3D molecules), `science/biology` (sequences, plasmids, trees, pathways, cells), `science/physics` (graphs and physics diagrams). Read the one you need before writing the fence.\nPass no path for the overview.",
        "parameters": {
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "e.g. \"design/principles\", \"design/formats/slides\", \"projects/existing-project\". Empty for the overview." }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_guide_named_in_the_tool_exists() {
        let names: Vec<&str> = EMBEDDED.iter().map(|(p, _)| *p).collect();
        for p in ["README.md", "design/index.md", "design/principles.md", "design/formats/pdf.md", "design/formats/slides.md", "design/formats/graphics.md", "design/templates/cv.html", "projects/index.md", "projects/new-project.md", "projects/existing-project.md", "projects/knowledge.md", "projects/working.md", "projects/roadmap.md", "projects/team.md", "science/chemistry.md", "science/biology.md", "science/physics.md", "science/index.md"] {
            assert!(names.contains(&p), "missing {p}");
        }
        assert!(EMBEDDED.iter().all(|(_, t)| !t.trim().is_empty()));
    }

    #[test]
    fn paths_resolve_flexibly_and_safely() {
        assert_eq!(candidates(""), vec!["README.md"]);
        assert!(candidates("design").contains(&"design/index.md".to_string()));
        assert!(candidates("design/templates/cv").contains(&"design/templates/cv.html".to_string()));
        assert!(read_local(Path::new("."), "../secret.md").is_none());
    }
}

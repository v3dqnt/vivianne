//! A governed way to get skills from NVIDIA and Nebius.
//!
//! Browse the two public skill catalogs (`NVIDIA/skills`, `nebius/skills`),
//! see a skill's card and what a static scan found, then install it with a
//! record of exactly where it came from: the commit and a SHA-256 for every
//! file. Later, `verify` re-hashes what's on disk, so a skill edited after
//! install is flagged rather than trusted.
//!
//! What this does not do: verify NVIDIA's signatures. Their `model-signing`
//! bundles need a sigstore verifier; until there is one here, a skill shows
//! as "from the NVIDIA catalog", not "verified". Installing adds files to
//! `~/.viv/skills` and nothing runs: a skill only reaches a prompt when the
//! user `@mentions` it.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::skill_scan::{scan, Report, SkillFile};

/// The sources offered: id, GitHub owner, repo.
const SOURCES: &[(&str, &str, &str)] = &[("nvidia", "NVIDIA", "skills"), ("nebius", "nebius", "skills")];

fn source(id: &str) -> Result<(&'static str, &'static str), String> {
    SOURCES.iter().find(|(s, _, _)| *s == id).map(|(_, o, r)| (*o, *r)).ok_or_else(|| format!("Unknown skill source: {id}"))
}

const MAX_FILES: usize = 60;
const MAX_FILE_BYTES: usize = 400_000;
const PROVENANCE: &str = ".provenance.json";

fn http() -> &'static reqwest::Client {
    static CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| reqwest::Client::builder().user_agent("Vivianne").timeout(Duration::from_secs(20)).build().expect("http client"));
    &CLIENT
}

// ── Reading the catalog ─────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct TreeEntry {
    path: String,
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    size: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CatalogEntry {
    pub name: String,
    /// Where the skill lives in the repo.
    pub dir: String,
    pub has_card: bool,
    pub files: usize,
    pub description: String,
}

const CARD_NAMES: &[&str] = &["skill-card.md", "skillcard.md", "skillcard.yaml", "skillcard.yml", "skill-card.yaml", "skill-card.yml"];

fn basename(p: &str) -> &str {
    p.rsplit('/').next().unwrap_or(p)
}

/// The skills in a repo tree: every folder that holds a `SKILL.md`.
fn catalog_from_tree(tree: &[TreeEntry], repo: &str) -> Vec<CatalogEntry> {
    let blobs: Vec<&TreeEntry> = tree.iter().filter(|e| e.kind == "blob").collect();
    let mut out = Vec::new();
    for e in &blobs {
        if basename(&e.path) != "SKILL.md" {
            continue;
        }
        let dir = e.path.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
        if dir.split('/').any(|s| s.starts_with('.') || s == "node_modules") {
            continue;
        }
        let prefix = if dir.is_empty() { String::new() } else { format!("{dir}/") };
        let inside: Vec<&&TreeEntry> = blobs.iter().filter(|b| b.path.starts_with(&prefix)).collect();
        out.push(CatalogEntry {
            name: if dir.is_empty() { repo.to_string() } else { basename(dir).to_string() },
            dir: dir.to_string(),
            has_card: inside.iter().any(|b| CARD_NAMES.contains(&basename(&b.path).to_ascii_lowercase().as_str())),
            files: inside.len(),
            description: String::new(),
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// A skill's description: the `description:` of its front matter, else the
/// first paragraph that isn't a heading.
fn description_of(md: &str) -> String {
    let mut lines = md.lines();
    if lines.next().map(str::trim) == Some("---") {
        let mut block = false;
        let mut found = String::new();
        for l in lines.by_ref() {
            if l.trim() == "---" {
                break;
            }
            if let Some(rest) = l.strip_prefix("description:") {
                let v = rest.trim().trim_matches(|c| c == '"' || c == '\'');
                if matches!(v, ">" | "|" | ">-" | "|-") {
                    block = true;
                } else {
                    found = v.to_string();
                }
            } else if block && (l.starts_with(' ') || l.starts_with('\t')) {
                if !found.is_empty() {
                    found.push(' ');
                }
                found.push_str(l.trim());
            } else if block && !l.trim().is_empty() {
                block = false;
            }
        }
        if !found.is_empty() {
            return found.chars().take(240).collect();
        }
    }
    md.lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with('#') && *l != "---" && !l.contains(':') || l.len() > 40 && !l.starts_with('#'))
        .map(|l| l.chars().take(240).collect())
        .unwrap_or_default()
}

/// A relative path that stays inside the skill's folder.
fn rel_ok(rel: &str) -> bool {
    !rel.is_empty()
        && rel.len() < 200
        && !rel.starts_with('/')
        && !rel.contains('\\')
        && !rel.contains(':')
        && rel.split('/').all(|s| !s.is_empty() && s != "." && s != "..")
}

fn safe_name(name: &str) -> bool {
    !name.is_empty() && name.len() < 80 && !name.starts_with('.') && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

// ── Network ─────────────────────────────────────────────────────────────────

async fn get_json(url: &str) -> Result<serde_json::Value, String> {
    let r = http().get(url).header("Accept", "application/vnd.github+json").send().await.map_err(|e| format!("Couldn't reach GitHub: {e}"))?;
    if r.status() == reqwest::StatusCode::FORBIDDEN || r.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
        return Err("GitHub's anonymous rate limit was reached; try again in a few minutes.".to_string());
    }
    if !r.status().is_success() {
        return Err(format!("GitHub answered {} for {url}", r.status()));
    }
    r.json().await.map_err(|e| format!("Unreadable answer from GitHub: {e}"))
}

async fn get_bytes(url: &str) -> Result<Vec<u8>, String> {
    let r = http().get(url).send().await.map_err(|e| format!("Couldn't download {url}: {e}"))?;
    if !r.status().is_success() {
        return Err(format!("{} for {url}", r.status()));
    }
    r.bytes().await.map(|b| b.to_vec()).map_err(|e| e.to_string())
}

/// The commit `main` is at, and the repo tree at it.
async fn repo_state(owner: &str, repo: &str) -> Result<(String, Vec<TreeEntry>), String> {
    let commit = get_json(&format!("https://api.github.com/repos/{owner}/{repo}/commits/main")).await?;
    let sha = commit.get("sha").and_then(|s| s.as_str()).ok_or("GitHub's answer had no commit")?.to_string();
    let tree = get_json(&format!("https://api.github.com/repos/{owner}/{repo}/git/trees/{sha}?recursive=1")).await?;
    let entries: Vec<TreeEntry> = serde_json::from_value(tree.get("tree").cloned().unwrap_or_default()).map_err(|e| format!("Unreadable repo listing: {e}"))?;
    Ok((sha, entries))
}

static CATALOG_CACHE: LazyLock<Mutex<HashMap<String, (Instant, serde_json::Value)>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

/// The skills a source offers, with descriptions. Cached for ten minutes.
#[tauri::command]
pub async fn cmd_skills_catalog(source_id: String) -> Result<serde_json::Value, String> {
    let (owner, repo) = source(&source_id)?;
    if let Some((at, v)) = CATALOG_CACHE.lock().unwrap_or_else(|e| e.into_inner()).get(&source_id) {
        if at.elapsed() < Duration::from_secs(600) {
            return Ok(v.clone());
        }
    }
    // Names and files come from the one repo listing, so this returns at once
    // even for hundreds of skills. Descriptions live in each SKILL.md and are
    // fetched for the ones on screen (`cmd_skills_describe`).
    let (commit, tree) = repo_state(owner, repo).await?;
    let entries = catalog_from_tree(&tree, repo);
    let value = serde_json::json!({ "source": source_id, "owner": owner, "repo": repo, "commit": commit, "skills": entries });
    CATALOG_CACHE.lock().unwrap_or_else(|e| e.into_inner()).insert(source_id, (Instant::now(), value.clone()));
    Ok(value)
}

static DESCRIPTIONS: LazyLock<Mutex<HashMap<String, String>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

/// Descriptions for some skills of a catalog, by folder. Fetched a dozen at a
/// time and remembered, so scrolling and searching cost nothing twice.
#[tauri::command]
pub async fn cmd_skills_describe(source_id: String, commit: String, dirs: Vec<String>) -> Result<HashMap<String, String>, String> {
    let (owner, repo) = source(&source_id)?;
    if commit.len() < 7 || !commit.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err("Not a commit.".to_string());
    }
    let mut out: HashMap<String, String> = HashMap::new();
    let mut todo: Vec<String> = Vec::new();
    for dir in dirs.into_iter().take(60) {
        if dir.split('/').any(|s| s == "..") {
            continue;
        }
        let key = format!("{source_id}@{commit}:{dir}");
        match DESCRIPTIONS.lock().unwrap_or_else(|e| e.into_inner()).get(&key) {
            Some(d) => {
                out.insert(dir, d.clone());
            }
            None => todo.push(dir),
        }
    }
    use futures::StreamExt;
    let jobs: Vec<(String, String)> = todo
        .into_iter()
        .map(|dir| {
            let path = if dir.is_empty() { "SKILL.md".to_string() } else { format!("{dir}/SKILL.md") };
            let url = format!("https://raw.githubusercontent.com/{owner}/{repo}/{commit}/{path}");
            (dir, url)
        })
        .collect();
    let fetched: Vec<(String, String)> = futures::stream::iter(jobs)
        .map(|(dir, url)| async move {
            let text = get_bytes(&url).await.map(|b| description_of(&String::from_utf8_lossy(&b))).unwrap_or_default();
            (dir, text)
        })
        .buffer_unordered(12)
        .collect()
        .await;
    let mut cache = DESCRIPTIONS.lock().unwrap_or_else(|e| e.into_inner());
    for (dir, text) in fetched {
        cache.insert(format!("{source_id}@{commit}:{dir}"), text.clone());
        out.insert(dir, text);
    }
    Ok(out)
}

struct Fetched {
    rel: String,
    bytes: Vec<u8>,
}

/// A skill's files at the repo's current commit. Binary files are listed but
/// not downloaded: the scan blocks them by name.
async fn fetch_skill(source_id: &str, dir: &str) -> Result<(String, Vec<Fetched>, Vec<(String, usize)>), String> {
    let (owner, repo) = source(source_id)?;
    if dir.split('/').any(|s| s == ".." || s.is_empty() && !dir.is_empty()) {
        return Err("Not a skill folder.".to_string());
    }
    let (commit, tree) = repo_state(owner, repo).await?;
    let prefix = if dir.is_empty() { String::new() } else { format!("{dir}/") };
    let mut listed: Vec<(String, usize)> = tree
        .iter()
        .filter(|e| e.kind == "blob" && e.path.starts_with(&prefix))
        .map(|e| (e.path[prefix.len()..].to_string(), e.size))
        .collect();
    if !listed.iter().any(|(p, _)| p == "SKILL.md") {
        return Err(format!("{dir} has no SKILL.md."));
    }
    if listed.len() > MAX_FILES {
        return Err(format!("This skill has {} files; skills here are limited to {MAX_FILES}.", listed.len()));
    }
    listed.sort();
    let mut files = Vec::new();
    for (rel, size) in &listed {
        if !rel_ok(rel) {
            return Err(format!("Unsafe file path in this skill: {rel}"));
        }
        let binary = ["exe", "dll", "so", "dylib", "msi", "bin", "jar", "wasm", "png", "jpg", "jpeg", "gif", "pdf", "zip", "gz", "pyc"].contains(&rel.rsplit('.').next().unwrap_or("").to_ascii_lowercase().as_str());
        if binary || *size > MAX_FILE_BYTES {
            continue;
        }
        let url = format!("https://raw.githubusercontent.com/{owner}/{repo}/{commit}/{prefix}{rel}");
        files.push(Fetched { rel: rel.clone(), bytes: get_bytes(&url).await? });
    }
    Ok((commit, files, listed))
}

fn to_scan_files(files: &[Fetched], listed: &[(String, usize)]) -> Vec<SkillFile> {
    let mut out: Vec<SkillFile> = files.iter().map(|f| SkillFile { path: f.rel.clone(), text: String::from_utf8_lossy(&f.bytes).to_string(), size: f.bytes.len() }).collect();
    // Listed but not downloaded (binaries, oversized): scanned by name and size.
    for (rel, size) in listed {
        if !files.iter().any(|f| &f.rel == rel) {
            out.push(SkillFile { path: rel.clone(), text: String::new(), size: *size });
        }
    }
    out
}

/// Everything the review sheet shows for one skill.
#[tauri::command]
pub async fn cmd_skills_preview(source_id: String, dir: String) -> Result<serde_json::Value, String> {
    let (commit, files, listed) = fetch_skill(&source_id, &dir).await?;
    let report = scan(&to_scan_files(&files, &listed));
    let text_of = |pred: &dyn Fn(&str) -> bool| files.iter().find(|f| pred(&f.rel)).map(|f| String::from_utf8_lossy(&f.bytes).to_string());
    Ok(serde_json::json!({
        "commit": commit,
        "name": if dir.is_empty() { source(&source_id)?.1.to_string() } else { basename(&dir).to_string() },
        "files": listed.iter().map(|(p, s)| serde_json::json!({ "path": p, "size": s })).collect::<Vec<_>>(),
        "skillMd": text_of(&|r| r == "SKILL.md"),
        "card": text_of(&|r| CARD_NAMES.contains(&basename(r).to_ascii_lowercase().as_str())),
        "report": report,
    }))
}

// ── Installing and verifying ────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Provenance {
    pub source: String,
    pub repo: String,
    pub dir: String,
    pub commit: String,
    pub installed_at: u64,
    /// File (relative to the skill folder) -> SHA-256.
    pub files: BTreeMap<String, String>,
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

/// Write a skill under `root/<name>` with its provenance manifest. Replaces
/// an earlier install of the same skill; refuses a folder that wasn't
/// installed from a catalog (it may be the user's own).
fn write_skill(root: &Path, name: &str, meta: &Provenance, files: &[Fetched]) -> Result<PathBuf, String> {
    if !safe_name(name) {
        return Err(format!("Not a usable skill name: {name}"));
    }
    let target = root.join(name);
    if target.exists() {
        if !target.join(PROVENANCE).is_file() {
            return Err(format!("A skill called {name} already exists and wasn't installed from a catalog; rename or remove it first."));
        }
        std::fs::remove_dir_all(&target).map_err(|e| format!("Couldn't replace the earlier install: {e}"))?;
    }
    for f in files {
        if !rel_ok(&f.rel) {
            return Err(format!("Unsafe file path: {}", f.rel));
        }
        let path = target.join(&f.rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::write(&path, &f.bytes).map_err(|e| format!("Couldn't write {}: {e}", f.rel))?;
    }
    let json = serde_json::to_string_pretty(meta).map_err(|e| e.to_string())?;
    std::fs::write(target.join(PROVENANCE), json).map_err(|e| e.to_string())?;
    Ok(target)
}

#[tauri::command]
pub async fn cmd_skills_install(source_id: String, dir: String) -> Result<serde_json::Value, String> {
    let (owner, repo) = source(&source_id)?;
    let (commit, files, listed) = fetch_skill(&source_id, &dir).await?;
    // Scanned again here, on what is about to be written: never on what the
    // review sheet showed earlier.
    let report: Report = scan(&to_scan_files(&files, &listed));
    if report.verdict == "block" {
        return Err("This skill was blocked by the scan, so it wasn't installed. Open the review to see why.".to_string());
    }
    let name = if dir.is_empty() { repo.to_string() } else { basename(&dir).to_string() };
    let meta = Provenance {
        source: source_id.clone(),
        repo: format!("{owner}/{repo}"),
        dir: dir.clone(),
        commit: commit.clone(),
        installed_at: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0),
        files: files.iter().map(|f| (f.rel.clone(), sha256_hex(&f.bytes))).collect(),
    };
    let root = crate::agent::system::paths::viv_skills_dir()?;
    std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    let target = write_skill(&root, &name, &meta, &files)?;
    Ok(serde_json::json!({ "name": name, "path": target.to_string_lossy(), "commit": commit, "verdict": report.verdict }))
}

#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct InstalledStatus {
    pub name: String,
    pub source: String,
    pub repo: String,
    pub commit: String,
    /// `ok`, `modified` or `missing`.
    pub status: &'static str,
    /// Files that differ from the manifest, were added, or are gone.
    pub changed: Vec<String>,
}

fn all_files(base: &Path, dir: &Path, out: &mut Vec<String>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            all_files(base, &p, out);
        } else if let Ok(rel) = p.strip_prefix(base) {
            let rel = rel.to_string_lossy().replace('\\', "/");
            if rel != PROVENANCE {
                out.push(rel);
            }
        }
    }
}

/// Compare an installed skill's files against its manifest.
fn verify_dir(path: &Path) -> Option<InstalledStatus> {
    let meta: Provenance = serde_json::from_str(&std::fs::read_to_string(path.join(PROVENANCE)).ok()?).ok()?;
    let mut changed = Vec::new();
    let mut missing = false;
    for (rel, want) in &meta.files {
        match std::fs::read(path.join(rel)) {
            Ok(bytes) if &sha256_hex(&bytes) == want => {}
            Ok(_) => changed.push(rel.clone()),
            Err(_) => {
                missing = true;
                changed.push(rel.clone());
            }
        }
    }
    let mut present = Vec::new();
    all_files(path, path, &mut present);
    for rel in present {
        if !meta.files.contains_key(&rel) {
            changed.push(rel);
        }
    }
    let status = if changed.is_empty() {
        "ok"
    } else if missing && !path.join("SKILL.md").exists() {
        "missing"
    } else {
        "modified"
    };
    Some(InstalledStatus { name: path.file_name()?.to_string_lossy().to_string(), source: meta.source, repo: meta.repo, commit: meta.commit, status, changed })
}

/// Skills installed from a catalog, each checked against its manifest.
#[tauri::command]
pub async fn cmd_skills_installed() -> Result<Vec<InstalledStatus>, String> {
    let root = crate::agent::system::paths::viv_skills_dir()?;
    let Ok(rd) = std::fs::read_dir(&root) else { return Ok(Vec::new()) };
    let mut out: Vec<InstalledStatus> = rd.flatten().filter(|e| e.path().is_dir()).filter_map(|e| verify_dir(&e.path())).collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

#[tauri::command]
pub async fn cmd_skills_remove(name: String) -> Result<(), String> {
    if !safe_name(&name) {
        return Err("Not a usable skill name.".to_string());
    }
    let target = crate::agent::system::paths::viv_skills_dir()?.join(&name);
    // Only what a catalog installed: never a skill the user wrote.
    if !target.join(PROVENANCE).is_file() {
        return Err("That skill wasn't installed from a catalog, so it isn't removed from here.".to_string());
    }
    std::fs::remove_dir_all(&target).map_err(|e| format!("Couldn't remove it: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(path: &str) -> TreeEntry {
        TreeEntry { path: path.to_string(), kind: "blob".to_string(), size: 10 }
    }
    fn temp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("viv_skills_hub_{tag}_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }
    fn fetched(rel: &str, text: &str) -> Fetched {
        Fetched { rel: rel.to_string(), bytes: text.as_bytes().to_vec() }
    }
    fn meta(files: &[Fetched]) -> Provenance {
        Provenance {
            source: "nvidia".into(),
            repo: "NVIDIA/skills".into(),
            dir: "skills/cuopt".into(),
            commit: "abc123".into(),
            installed_at: 1,
            files: files.iter().map(|f| (f.rel.clone(), sha256_hex(&f.bytes))).collect(),
        }
    }

    #[test]
    fn skills_are_the_folders_that_hold_a_skill_md() {
        let tree = vec![
            entry("README.md"),
            entry("skills/cuopt/SKILL.md"),
            entry("skills/cuopt/SKILLCARD.yaml"),
            entry("skills/cuopt/scripts/run.py"),
            entry("skills/nemo/SKILL.md"),
            entry(".github/skills/x/SKILL.md"),
            TreeEntry { path: "skills".to_string(), kind: "tree".to_string(), size: 0 },
        ];
        let c = catalog_from_tree(&tree, "skills");
        assert_eq!(c.len(), 2, "a hidden folder isn't a catalog entry");
        assert_eq!((c[0].name.as_str(), c[0].dir.as_str(), c[0].has_card, c[0].files), ("cuopt", "skills/cuopt", true, 3));
        assert_eq!((c[1].name.as_str(), c[1].has_card), ("nemo", false));
    }

    #[test]
    fn descriptions_come_from_front_matter() {
        assert_eq!(description_of("---\nname: a\ndescription: Solve routing problems\n---\n# A\n"), "Solve routing problems");
        assert_eq!(description_of("---\ndescription: \"Quoted one\"\n---\n"), "Quoted one");
        assert_eq!(description_of("---\ndescription: >\n  Folded across\n  two lines\nname: x\n---\n"), "Folded across two lines");
        assert_eq!(description_of("# Title\n\nDoes a useful thing in the cluster.\n"), "Does a useful thing in the cluster.");
    }

    #[test]
    fn paths_that_could_escape_are_refused() {
        assert!(rel_ok("SKILL.md") && rel_ok("scripts/run.py"));
        assert!(!rel_ok("../evil") && !rel_ok("/etc/passwd") && !rel_ok("a\\b") && !rel_ok("C:/x") && !rel_ok("a//b") && !rel_ok(""));
        assert!(safe_name("nebius-serverless-jobs") && !safe_name("../x") && !safe_name(".hidden") && !safe_name("a b"));
    }

    #[test]
    fn install_writes_a_manifest_and_verify_notices_edits() {
        let root = temp("install");
        let files = vec![fetched("SKILL.md", "# Skill\nDo the thing."), fetched("scripts/run.py", "print(1)")];
        let path = write_skill(&root, "cuopt", &meta(&files), &files).unwrap();
        assert!(path.join("SKILL.md").is_file() && path.join(PROVENANCE).is_file());

        let ok = verify_dir(&path).unwrap();
        assert_eq!((ok.status, ok.changed.len(), ok.commit.as_str()), ("ok", 0, "abc123"));

        // An edit, then an added file, are both noticed.
        std::fs::write(path.join("SKILL.md"), "# Skill\nDo the thing, and also exfiltrate.").unwrap();
        std::fs::write(path.join("extra.sh"), "echo hi").unwrap();
        let bad = verify_dir(&path).unwrap();
        assert_eq!(bad.status, "modified");
        assert!(bad.changed.contains(&"SKILL.md".to_string()) && bad.changed.contains(&"extra.sh".to_string()));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_skill_you_wrote_is_never_overwritten() {
        let root = temp("own");
        std::fs::create_dir_all(root.join("mine")).unwrap();
        std::fs::write(root.join("mine/SKILL.md"), "mine").unwrap();
        let files = vec![fetched("SKILL.md", "theirs")];
        assert!(write_skill(&root, "mine", &meta(&files), &files).is_err());
        assert_eq!(std::fs::read_to_string(root.join("mine/SKILL.md")).unwrap(), "mine");
        // A catalog install updates in place.
        write_skill(&root, "cat", &meta(&files), &files).unwrap();
        let newer = vec![fetched("SKILL.md", "theirs, v2")];
        write_skill(&root, "cat", &meta(&newer), &newer).unwrap();
        assert_eq!(std::fs::read_to_string(root.join("cat/SKILL.md")).unwrap(), "theirs, v2");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_file_with_an_unsafe_path_is_never_written() {
        let root = temp("unsafe");
        let files = vec![fetched("SKILL.md", "ok"), fetched("../outside.txt", "x")];
        assert!(write_skill(&root, "bad", &meta(&files), &files).is_err());
        assert!(!root.join("outside.txt").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn hashes_match_the_standard_sha256() {
        assert_eq!(sha256_hex(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }
}

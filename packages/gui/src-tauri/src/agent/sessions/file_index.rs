//! A ranked index of the files in a workspace, for `@` mentions in the composer.
//!
//! Built once per folder (up to 50,000 entries, honouring the project's own `.gitignore`), then kept. When it gets old the next search still
//! answers from the old index at once while a fresh one is built in the background, so typing never waits on a disk walk. Matches are ranked:
//! an exact file name first, then a name that starts with the query, a name that contains it, a path that contains it, and last a path that
//! has the query's letters in order (`ccm` finds `ChatComposer.tsx`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::Serialize;

const MAX_ENTRIES: usize = 50_000;
const MAX_DEPTH: usize = 12;
/// How long an index is trusted before the next search starts rebuilding it (the old one keeps answering meanwhile).
const STALE_AFTER: Duration = Duration::from_secs(30);

#[derive(Debug, Clone)]
pub struct Entry {
    /// Relative to the folder, with forward slashes.
    pub rel: String,
    pub name: String,
    pub is_dir: bool,
    rel_lc: String,
    name_lc: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Match {
    pub rel_path: String,
    pub name: String,
    pub is_dir: bool,
    /// What to write after the `@`: the path relative to its folder, or the full path when several folders are open.
    pub insert_path: String,
}

struct Slot {
    entries: Arc<Vec<Entry>>,
    built: Instant,
    building: bool,
}

fn slots() -> &'static Mutex<HashMap<PathBuf, Slot>> {
    static S: OnceLock<Mutex<HashMap<PathBuf, Slot>>> = OnceLock::new();
    S.get_or_init(Default::default)
}

/// Walk a folder into entries, skipping what the project ignores and dot-folders.
pub fn build(root: &Path) -> Vec<Entry> {
    let ignore = crate::agent::tools::builtin::IgnoreSet::from_root(root);
    let mut out = Vec::new();
    let mut stack = vec![(root.to_path_buf(), 0usize)];
    while let Some((dir, depth)) = stack.pop() {
        let Ok(read) = std::fs::read_dir(&dir) else { continue };
        for e in read.flatten() {
            if out.len() >= MAX_ENTRIES {
                return out;
            }
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                continue;
            }
            let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
            let path = e.path();
            let rel = path.strip_prefix(root).unwrap_or(&path).to_string_lossy().replace('\\', "/");
            if ignore.is_ignored(&rel, is_dir) {
                continue;
            }
            if is_dir && depth + 1 < MAX_DEPTH {
                stack.push((path, depth + 1));
            }
            out.push(Entry { rel_lc: rel.to_lowercase(), name_lc: name.to_lowercase(), rel, name, is_dir });
        }
    }
    out
}

/// How well `entry` matches the lower-cased `q`: lower is better, `None` is no match.
fn score(q: &str, e: &Entry) -> Option<u32> {
    let tier = if e.name_lc == q {
        0
    } else if e.name_lc.starts_with(q) {
        1
    } else if e.name_lc.contains(q) {
        2
    } else if e.rel_lc.contains(q) {
        3
    } else if in_order(q, &e.rel_lc) {
        4
    } else {
        return None;
    };
    // Within a tier: a folder slightly ahead of a file (it opens a whole area), then the shorter, shallower path.
    Some(tier * 10_000 + u32::from(!e.is_dir) * 2_000 + (e.rel.len().min(1_900) as u32))
}

/// Whether every character of `q` appears in `s` in the same order.
fn in_order(q: &str, s: &str) -> bool {
    let mut it = s.chars();
    q.chars().all(|c| it.any(|x| x == c))
}

/// The best `limit` entries for `query`, best first. An empty query lists the shallowest entries.
pub fn rank<'a>(entries: &'a [Entry], query: &str, limit: usize) -> Vec<&'a Entry> {
    let q = query.trim().trim_start_matches("./").to_lowercase();
    let mut scored: Vec<(u32, &Entry)> = if q.is_empty() {
        entries.iter().map(|e| (e.rel.matches('/').count() as u32 * 1000 + e.rel.len() as u32, e)).collect()
    } else {
        entries.iter().filter_map(|e| score(&q, e).map(|s| (s, e))).collect()
    };
    scored.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.rel.cmp(&b.1.rel)));
    scored.into_iter().take(limit).map(|(_, e)| e).collect()
}

/// The index for a folder: built now if there is none, served as it is if fresh, and served as it is while a rebuild runs if stale.
async fn index_for(root: &Path) -> Arc<Vec<Entry>> {
    let existing = {
        let mut map = slots().lock().unwrap_or_else(|e| e.into_inner());
        map.get_mut(root).map(|s| {
            let stale = s.built.elapsed() > STALE_AFTER && !s.building;
            if stale {
                s.building = true;
            }
            (s.entries.clone(), stale)
        })
    };
    match existing {
        Some((entries, stale)) => {
            if stale {
                let root = root.to_path_buf();
                tokio::task::spawn_blocking(move || {
                    let fresh = Arc::new(build(&root));
                    slots().lock().unwrap_or_else(|e| e.into_inner()).insert(root, Slot { entries: fresh, built: Instant::now(), building: false });
                });
            }
            entries
        }
        None => {
            let root = root.to_path_buf();
            let built = tokio::task::spawn_blocking({
                let root = root.clone();
                move || Arc::new(build(&root))
            })
            .await
            .unwrap_or_default();
            slots().lock().unwrap_or_else(|e| e.into_inner()).insert(root, Slot { entries: built.clone(), built: Instant::now(), building: false });
            built
        }
    }
}

/// Files and folders matching `query` across the open folders, ranked.
#[tauri::command]
pub async fn cmd_file_search(roots: Vec<String>, query: String, limit: Option<usize>) -> Result<Vec<Match>, String> {
    let limit = limit.unwrap_or(20).clamp(1, 100);
    let multi = roots.len() > 1;
    let mut all: Vec<(u32, Match)> = Vec::new();
    for root in &roots {
        let path = PathBuf::from(root);
        if !path.is_dir() {
            continue;
        }
        let entries = index_for(&path).await;
        let folder = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let q = query.trim().trim_start_matches("./").to_lowercase();
        for e in rank(&entries, &query, limit) {
            let s = if q.is_empty() { e.rel.matches('/').count() as u32 * 1000 } else { score(&q, e).unwrap_or(u32::MAX) };
            all.push((
                s,
                Match {
                    rel_path: if multi { format!("{folder}/{}", e.rel) } else { e.rel.clone() },
                    name: e.name.clone(),
                    is_dir: e.is_dir,
                    insert_path: if multi { path.join(&e.rel).to_string_lossy().replace('\\', "/") } else { e.rel.clone() },
                },
            ));
        }
    }
    all.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.rel_path.cmp(&b.1.rel_path)));
    Ok(all.into_iter().take(limit).map(|(_, m)| m).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(rel: &str, is_dir: bool) -> Entry {
        let name = rel.rsplit('/').next().unwrap().to_string();
        Entry { rel: rel.to_string(), rel_lc: rel.to_lowercase(), name_lc: name.to_lowercase(), name, is_dir }
    }

    fn names(found: Vec<&Entry>) -> Vec<String> {
        found.into_iter().map(|e| e.rel.clone()).collect()
    }

    #[test]
    fn an_exact_name_beats_a_prefix_a_substring_a_path_and_a_scatter() {
        let all = vec![
            entry("src/deep/mentions-notes.md", false),
            entry("lib/mentions.ts", false),
            entry("mentions/readme.md", false),
            entry("docs/Mentions.md", false),
            entry("m/e/n/t/i/o/n/s.txt", false),
        ];
        let got = names(rank(&all, "mentions.ts", 10));
        assert_eq!(got[0], "lib/mentions.ts");
        let got = names(rank(&all, "mentions", 10));
        // The three names that start with it come before the one that only has it in its folder name.
        assert_eq!(got[3], "mentions/readme.md");
        assert_eq!(got[4], "m/e/n/t/i/o/n/s.txt", "scattered letters come last");
    }

    #[test]
    fn letters_in_order_find_a_file() {
        let all = vec![entry("src/components/ChatComposer.tsx", false), entry("src/other.ts", false)];
        assert_eq!(names(rank(&all, "ccm", 5)), ["src/components/ChatComposer.tsx"]);
        assert!(rank(&all, "zzz", 5).is_empty());
    }

    #[test]
    fn within_a_tier_a_folder_and_a_shorter_path_come_first() {
        let all = vec![entry("a/b/c/tool.rs", false), entry("tool.rs", false), entry("tools", true)];
        let got = names(rank(&all, "tool", 10));
        assert_eq!(got[0], "tools", "a folder first");
        assert_eq!(got[1], "tool.rs");
    }

    #[test]
    fn the_result_is_limited_and_an_empty_query_lists_the_shallowest() {
        let all: Vec<Entry> = (0..50).map(|i| entry(&format!("f{i}.txt"), false)).chain(std::iter::once(entry("a/b/c/d.txt", false))).collect();
        assert_eq!(rank(&all, "f", 20).len(), 20);
        let first = names(rank(&all, "", 3));
        assert!(!first.contains(&"a/b/c/d.txt".to_string()));
    }

    #[test]
    fn building_honours_gitignore_and_skips_dot_folders() {
        let dir = std::env::temp_dir().join(format!("viv-index-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::create_dir_all(dir.join("build")).unwrap();
        std::fs::create_dir_all(dir.join(".hidden")).unwrap();
        std::fs::write(dir.join(".gitignore"), "build/\n").unwrap();
        std::fs::write(dir.join("src/app.ts"), "").unwrap();
        std::fs::write(dir.join("build/out.js"), "").unwrap();
        std::fs::write(dir.join(".hidden/x.txt"), "").unwrap();
        let rels: Vec<String> = build(&dir).into_iter().map(|e| e.rel).collect();
        assert!(rels.contains(&"src/app.ts".to_string()));
        assert!(!rels.iter().any(|r| r.starts_with("build") || r.starts_with(".hidden")), "{rels:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

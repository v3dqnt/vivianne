//! Applying what a language server sends back as edits: `WorkspaceEdit` (rename, code actions, server-initiated `workspace/applyEdit`) and
//! `TextEdit[]` (formatting), plus the formatting options a file asks for.
//!
//! All or nothing: every file's new content is worked out and every path checked against the workspace before anything is written, and each write
//! goes through the file-safety layer's atomic replace. A path outside the workspace refuses the whole edit.

use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::uri::{path_key, uri_to_path};

/// One planned change to the disk.
#[derive(Debug, PartialEq)]
enum Step {
    Write(PathBuf, String),
    Create(PathBuf),
    Rename(PathBuf, PathBuf),
    Delete(PathBuf),
}

/// Whether `path` is the workspace `root` or inside it: lexically (so `..` cannot walk out), and, for what exists, by real path (so a symlink cannot).
pub fn inside_root(path: &Path, root: &str) -> bool {
    let (p, r) = (path_key(&path.to_string_lossy()), path_key(root));
    if p != r && !p.starts_with(&format!("{}/", r.trim_end_matches('/'))) {
        return false;
    }
    // The nearest part that exists decides the real location.
    let mut existing = path;
    while !existing.exists() {
        match existing.parent() {
            Some(parent) => existing = parent,
            None => return true,
        }
    }
    match (std::fs::canonicalize(existing), std::fs::canonicalize(root)) {
        (Ok(a), Ok(b)) => a.starts_with(b),
        _ => true,
    }
}

/// Byte offset of an LSP position (UTF-16 units) in `text`, clamped to the line's end the way servers expect.
fn offset_of(text: &str, line: u32, character: u32) -> usize {
    let mut start = 0usize;
    for _ in 0..line {
        match text[start..].find('\n') {
            Some(i) => start += i + 1,
            None => return text.len(),
        }
    }
    let line_end = text[start..].find('\n').map(|i| start + i).unwrap_or(text.len());
    let body_end = if text[start..line_end].ends_with('\r') { line_end - 1 } else { line_end };
    let mut units = 0u32;
    for (i, c) in text[start..body_end].char_indices() {
        if units >= character {
            return start + i;
        }
        units += c.len_utf16() as u32;
    }
    body_end
}

/// `edits` (a `TextEdit[]`) applied to `text`. Edits are in terms of the original text and must not overlap.
pub fn apply_text_edits(text: &str, edits: &[Value]) -> Result<String, String> {
    let mut spans: Vec<(usize, usize, String)> = Vec::new();
    for e in edits {
        let pos = |p: &str, k: &str| e.pointer(&format!("/range/{p}/{k}")).and_then(|v| v.as_u64()).map(|v| v as u32);
        let (Some(sl), Some(sc), Some(el), Some(ec)) = (pos("start", "line"), pos("start", "character"), pos("end", "line"), pos("end", "character")) else {
            return Err("an edit has no range".into());
        };
        let new_text = e.get("newText").and_then(|t| t.as_str()).ok_or("an edit has no newText")?;
        let (a, b) = (offset_of(text, sl, sc), offset_of(text, el, ec));
        if b < a {
            return Err("an edit's range ends before it starts".into());
        }
        spans.push((a, b, new_text.to_string()));
    }
    // Stable: edits at one point keep the server's order.
    spans.sort_by_key(|s| (s.0, s.1));
    for w in spans.windows(2) {
        if w[0].1 > w[1].0 {
            return Err("the server sent edits that overlap".into());
        }
    }
    let mut out = String::with_capacity(text.len());
    let mut at = 0usize;
    for (a, b, new_text) in spans {
        out.push_str(&text[at..a]);
        out.push_str(&new_text);
        at = b;
    }
    out.push_str(&text[at..]);
    Ok(out)
}

fn plan(root: &str, edit: &Value) -> Result<Vec<Step>, String> {
    let checked = |uri: &str| -> Result<PathBuf, String> {
        let p = PathBuf::from(uri_to_path(uri));
        if inside_root(&p, root) {
            Ok(p)
        } else {
            Err(format!("the server tried to change {}, which is outside the workspace", p.display()))
        }
    };
    // The text each file will have, so several edits to one file stack instead of each reading the original.
    let mut texts: BTreeMap<String, (PathBuf, String)> = BTreeMap::new();
    let mut steps: Vec<Step> = Vec::new();
    let edit_file = |texts: &mut BTreeMap<String, (PathBuf, String)>, uri: &str, edits: &[Value]| -> Result<(), String> {
        let path = checked(uri)?;
        let key = path_key(uri);
        let current = match texts.get(&key) {
            Some((_, t)) => t.clone(),
            None => std::fs::read_to_string(&path).map_err(|e| format!("can't read {}: {e}", path.display()))?,
        };
        let next = apply_text_edits(&current, edits)?;
        texts.insert(key, (path, next));
        Ok(())
    };

    if let Some(changes) = edit.get("changes").and_then(|c| c.as_object()) {
        for (uri, edits) in changes {
            edit_file(&mut texts, uri, edits.as_array().map(|a| a.as_slice()).unwrap_or(&[]))?;
        }
    }
    if let Some(doc_changes) = edit.get("documentChanges").and_then(|c| c.as_array()) {
        for change in doc_changes {
            match change.get("kind").and_then(|k| k.as_str()) {
                Some("create") => {
                    let path = checked(change["uri"].as_str().ok_or("a create has no uri")?)?;
                    if path.exists() && !change.pointer("/options/overwrite").and_then(|v| v.as_bool()).unwrap_or(false) && !change.pointer("/options/ignoreIfExists").and_then(|v| v.as_bool()).unwrap_or(false) {
                        return Err(format!("{} already exists", path.display()));
                    }
                    if !path.exists() {
                        steps.push(Step::Create(path));
                    }
                }
                Some("rename") => {
                    let from = checked(change["oldUri"].as_str().ok_or("a rename has no oldUri")?)?;
                    let to = checked(change["newUri"].as_str().ok_or("a rename has no newUri")?)?;
                    if to.exists() && !change.pointer("/options/overwrite").and_then(|v| v.as_bool()).unwrap_or(false) {
                        return Err(format!("{} already exists", to.display()));
                    }
                    // Edits already planned for the old path follow the file.
                    if let Some((_, (_, text))) = texts.remove_entry(&path_key(&from.to_string_lossy())) {
                        steps.push(Step::Write(from.clone(), text));
                    }
                    steps.push(Step::Rename(from, to));
                }
                Some("delete") => steps.push(Step::Delete(checked(change["uri"].as_str().ok_or("a delete has no uri")?)?)),
                _ => {
                    let uri = change.pointer("/textDocument/uri").and_then(|u| u.as_str()).ok_or("a document change has no uri")?;
                    let edits: Vec<Value> = change.get("edits").and_then(|e| e.as_array()).cloned().unwrap_or_default();
                    // An annotated edit carries the same range and newText.
                    edit_file(&mut texts, uri, &edits)?;
                }
            }
        }
    }
    // Edits first, then the file operations that followed them, in the order given.
    let mut all: Vec<Step> = texts.into_values().map(|(p, t)| Step::Write(p, t)).collect();
    all.extend(steps);
    Ok(all)
}

/// Apply a `WorkspaceEdit` under `root`. Returns what changed, one line per file.
pub async fn apply_workspace_edit(root: &str, edit: &Value) -> Result<Vec<String>, String> {
    let steps = plan(root, edit)?;
    let mut done = Vec::new();
    for step in steps {
        match step {
            Step::Write(path, text) => {
                crate::agent::tools::files::guard::write_atomic(&path, text.as_bytes()).await.map_err(|e| format!("can't write {}: {e}", path.display()))?;
                done.push(format!("edited {}", path.display()));
            }
            Step::Create(path) => {
                if let Some(dir) = path.parent() {
                    tokio::fs::create_dir_all(dir).await.map_err(|e| e.to_string())?;
                }
                tokio::fs::write(&path, b"").await.map_err(|e| format!("can't create {}: {e}", path.display()))?;
                done.push(format!("created {}", path.display()));
            }
            Step::Rename(from, to) => {
                if let Some(dir) = to.parent() {
                    tokio::fs::create_dir_all(dir).await.map_err(|e| e.to_string())?;
                }
                tokio::fs::rename(&from, &to).await.map_err(|e| format!("can't rename {}: {e}", from.display()))?;
                done.push(format!("renamed {} to {}", from.display(), to.display()));
            }
            Step::Delete(path) => {
                if path.is_dir() {
                    tokio::fs::remove_dir_all(&path).await
                } else {
                    tokio::fs::remove_file(&path).await
                }
                .map_err(|e| format!("can't delete {}: {e}", path.display()))?;
                done.push(format!("deleted {}", path.display()));
            }
        }
    }
    Ok(done)
}

/// The paths a `WorkspaceEdit` touches, for telling the server and for the result.
pub fn touched(edit: &Value) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    if let Some(c) = edit.get("changes").and_then(|c| c.as_object()) {
        out.extend(c.keys().map(|u| uri_to_path(u)));
    }
    for ch in edit.get("documentChanges").and_then(|c| c.as_array()).into_iter().flatten() {
        if let Some(u) = ch.pointer("/textDocument/uri").or_else(|| ch.get("uri")).or_else(|| ch.get("newUri")).and_then(|u| u.as_str()) {
            out.push(uri_to_path(u));
        }
    }
    out
}

// ── Formatting options ──────────────────────────────────────────────────────

/// `FormattingOptions` for `path`: `.editorconfig` (nearest first, up to the workspace root), else what the file already does.
pub fn formatting_options(path: &Path, content: &str, root: &str) -> Value {
    let (mut use_tabs, mut size): (Option<bool>, Option<u32>) = (None, None);
    let mut trim: Option<bool> = None;
    let mut final_nl: Option<bool> = None;
    let mut dir = path.parent();
    let mut chain: Vec<PathBuf> = Vec::new();
    while let Some(d) = dir {
        chain.push(d.join(".editorconfig"));
        if path_key(&d.to_string_lossy()) == path_key(root) {
            break;
        }
        dir = d.parent();
    }
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    for file in chain {
        let Ok(text) = std::fs::read_to_string(&file) else { continue };
        let mut applies = false;
        let mut root_file = false;
        // Within one file a later matching section wins; a nearer file wins over a farther one.
        let (mut f_tabs, mut f_size, mut f_trim, mut f_nl): (Option<bool>, Option<u32>, Option<bool>, Option<bool>) = (None, None, None, None);
        for line in text.lines().map(str::trim) {
            if line.starts_with('#') || line.starts_with(';') || line.is_empty() {
                continue;
            }
            if let Some(glob) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
                applies = glob_matches(glob, &name);
            } else if let Some((k, v)) = line.split_once('=') {
                let (k, v) = (k.trim().to_lowercase(), v.trim().to_lowercase());
                if !applies {
                    if k == "root" && v == "true" {
                        root_file = true;
                    }
                    continue;
                }
                match k.as_str() {
                    "indent_style" => f_tabs = Some(v == "tab"),
                    "indent_size" | "tab_width" => f_size = v.parse().ok().or(f_size),
                    "trim_trailing_whitespace" => f_trim = Some(v == "true"),
                    "insert_final_newline" => f_nl = Some(v == "true"),
                    _ => {}
                }
            }
        }
        use_tabs = use_tabs.or(f_tabs);
        size = size.or(f_size);
        trim = trim.or(f_trim);
        final_nl = final_nl.or(f_nl);
        if root_file {
            break;
        }
    }
    let (sniff_tabs, sniff_size) = sniff_indent(content);
    let mut o = json!({ "tabSize": size.or(sniff_size).unwrap_or(4), "insertSpaces": !use_tabs.unwrap_or(sniff_tabs) });
    if let Some(t) = trim {
        o["trimTrailingWhitespace"] = json!(t);
    }
    if let Some(n) = final_nl {
        o["insertFinalNewline"] = json!(n);
    }
    o
}

/// The `[glob]` forms an `.editorconfig` uses most: `*`, `*.ext`, `*.{a,b}`, an exact name.
fn glob_matches(glob: &str, name: &str) -> bool {
    glob.split(',').flat_map(|g| expand_braces(g.trim())).any(|g| {
        let g = g.trim_start_matches("**/");
        if g == "*" {
            return true;
        }
        match g.strip_prefix("*.") {
            Some(ext) => name.ends_with(&format!(".{ext}")),
            None => g == name,
        }
    })
}

fn expand_braces(g: &str) -> Vec<String> {
    match (g.find('{'), g.find('}')) {
        (Some(a), Some(b)) if a < b => g[a + 1..b].split(',').map(|alt| format!("{}{}{}", &g[..a], alt, &g[b + 1..])).collect(),
        _ => vec![g.to_string()],
    }
}

/// Tabs or spaces, and how wide, from the file's own lines: the commonest step between one line's indent and the next.
fn sniff_indent(content: &str) -> (bool, Option<u32>) {
    let (mut tabs, mut spaces) = (0u32, 0u32);
    let mut steps: BTreeMap<u32, u32> = BTreeMap::new();
    let mut prev = 0u32;
    for line in content.lines().take(400) {
        if line.trim().is_empty() {
            continue;
        }
        if line.starts_with('\t') {
            tabs += 1;
        } else if line.starts_with(' ') {
            spaces += 1;
            let w = line.len() - line.trim_start_matches(' ').len();
            let w = w as u32;
            if w > prev {
                *steps.entry(w - prev).or_default() += 1;
            }
            prev = w;
            continue;
        }
        prev = 0;
    }
    let width = steps.into_iter().filter(|(s, _)| (1..=8).contains(s)).max_by_key(|(_, n)| *n).map(|(s, _)| s);
    (tabs > spaces, width)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> PathBuf {
        let d = std::env::temp_dir().join(format!("viv-edits-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn range(sl: u32, sc: u32, el: u32, ec: u32) -> Value {
        json!({ "start": { "line": sl, "character": sc }, "end": { "line": el, "character": ec } })
    }

    #[test]
    fn edits_apply_against_the_original_text_in_utf16_columns() {
        let text = "let a😀 = 1;\nlet b = 2;\n";
        // "a😀" is a, then two UTF-16 units: the `=` starts at column 8.
        let edits = vec![json!({ "range": range(0, 4, 0, 7), "newText": "x" }), json!({ "range": range(1, 4, 1, 5), "newText": "bee" })];
        assert_eq!(apply_text_edits(text, &edits).unwrap(), "let x = 1;\nlet bee = 2;\n");
        // Given out of order, and an insert at the very end.
        let edits = vec![json!({ "range": range(1, 11, 1, 11), "newText": " // end" }), json!({ "range": range(0, 0, 0, 3), "newText": "const" })];
        assert_eq!(apply_text_edits(text, &edits).unwrap(), "const a😀 = 1;\nlet b = 2; // end\n");
    }

    #[test]
    fn overlapping_edits_are_refused() {
        let edits = vec![json!({ "range": range(0, 0, 0, 4), "newText": "a" }), json!({ "range": range(0, 2, 0, 6), "newText": "b" })];
        assert!(apply_text_edits("abcdefgh", &edits).unwrap_err().contains("overlap"));
    }

    #[test]
    fn crlf_lines_end_before_the_carriage_return() {
        let edits = vec![json!({ "range": range(0, 3, 0, 99), "newText": "X" })];
        assert_eq!(apply_text_edits("abc\r\ndef", &edits).unwrap(), "abcX\r\ndef");
    }

    #[tokio::test]
    async fn a_workspace_edit_changes_creates_renames_and_deletes() {
        let root = tmp();
        let (a, gone, moved) = (root.join("a.txt"), root.join("gone.txt"), root.join("old.txt"));
        std::fs::write(&a, "hello world").unwrap();
        std::fs::write(&gone, "x").unwrap();
        std::fs::write(&moved, "m").unwrap();
        let edit = json!({ "documentChanges": [
            { "textDocument": { "uri": super::super::uri::path_to_uri(&a.to_string_lossy()), "version": null }, "edits": [{ "range": range(0, 0, 0, 5), "newText": "bye" }] },
            { "textDocument": { "uri": super::super::uri::path_to_uri(&a.to_string_lossy()), "version": null }, "edits": [{ "range": range(0, 6, 0, 11), "newText": "all" }] },
            { "kind": "create", "uri": super::super::uri::path_to_uri(&root.join("sub/new.txt").to_string_lossy()) },
            { "kind": "delete", "uri": super::super::uri::path_to_uri(&gone.to_string_lossy()) },
            { "kind": "rename", "oldUri": super::super::uri::path_to_uri(&moved.to_string_lossy()), "newUri": super::super::uri::path_to_uri(&root.join("new.txt").to_string_lossy()) },
        ] });
        let done = apply_workspace_edit(&root.to_string_lossy(), &edit).await.unwrap();
        assert_eq!(done.len(), 4, "{done:?}");
        // Two document changes to one file stack; the second's columns are about the text the first left.
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "bye all");
        assert!(root.join("sub/new.txt").exists() && !gone.exists() && !moved.exists() && root.join("new.txt").exists());
        std::fs::remove_dir_all(&root).ok();
    }

    #[tokio::test]
    async fn a_path_outside_the_workspace_refuses_the_whole_edit() {
        let root = tmp();
        let inside = root.join("in.txt");
        std::fs::write(&inside, "keep").unwrap();
        let outside = tmp().join("out.txt");
        std::fs::write(&outside, "keep").unwrap();
        let sneaky = format!("{}/../{}", root.display(), outside.file_name().unwrap().to_string_lossy());
        for target in [outside.to_string_lossy().to_string(), sneaky] {
            let edit = json!({ "changes": {
                super::super::uri::path_to_uri(&inside.to_string_lossy()): [{ "range": range(0, 0, 0, 4), "newText": "changed" }],
                super::super::uri::path_to_uri(&target): [{ "range": range(0, 0, 0, 4), "newText": "changed" }],
            } });
            let err = apply_workspace_edit(&root.to_string_lossy(), &edit).await.unwrap_err();
            assert!(err.contains("outside the workspace"), "{err}");
            assert_eq!(std::fs::read_to_string(&inside).unwrap(), "keep", "nothing is written when any path is refused");
        }
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn editorconfig_decides_and_the_file_decides_otherwise() {
        let root = tmp();
        std::fs::write(root.join(".editorconfig"), "root = true\n[*]\nindent_style = space\nindent_size = 2\n[*.go]\nindent_style = tab\n").unwrap();
        let ts = root.join("a.ts");
        assert_eq!(formatting_options(&ts, "", &root.to_string_lossy()), json!({ "tabSize": 2, "insertSpaces": true }));
        let go = root.join("a.go");
        assert_eq!(formatting_options(&go, "", &root.to_string_lossy())["insertSpaces"], false);
        // No config: sniffed from the content.
        let bare = tmp();
        let o = formatting_options(&bare.join("b.py"), "def f():\n    x = 1\n    if x:\n        y = 2\n", &bare.to_string_lossy());
        assert_eq!(o, json!({ "tabSize": 4, "insertSpaces": true }));
        let o = formatting_options(&bare.join("c.c"), "int f() {\n\treturn 1;\n}\n", &bare.to_string_lossy());
        assert_eq!(o["insertSpaces"], false);
        std::fs::remove_dir_all(&root).ok();
    }
}

//! Enhanced read/list tools — ports grok's richer file surface onto Vivianne:
//!
//! - `read_file` — cursor/ripgrep-style "smart" reader that detects PDFs (byte
//!   count + page count via cheap header scan), image files (dimensions +
//!   format via the existing `image` crate), Jupyter notebooks (structured
//!   cell summary instead of raw JSON), and applies a gitignore-aware
//!   line-numbered slice for regular text files.
//! - `list_dir` — hierarchical tree walk, gitignore-aware, with a budget
//!   notice when nodes are pruned, mirroring grok's `list_dir` v2 output.
//!
//! Vivianne's original `read` / `ls` / `find` / `glob` / `grep` are preserved
//! unchanged as the "raw" tools; these are the higher-level convenience
//! surface, same split grok has between the low-level codex tools and the
//! OpenCode wrappers.

use async_trait::async_trait;
use serde_json::json;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use super::tool_exec::{resolve_path, truncate_text, Tool, ToolResult};

// ── Gitignore helper ─────────────────────────────────────────────────────────
//
// Vivianne doesn't take a hard dep on the `ignore` crate; the cost of the
// binary is not worth it for the simple case here. Instead we read `.gitignore`
// at the search root and honor the small subset actually used in real repos:
// blank lines, `#` comments, `!` negation, trailing `/` (directory only),
// leading `/` (root-anchored), and simple `*` / `?` globs.

#[derive(Debug, Clone)]
struct IgnoreRule {
    pattern: regex::Regex,
    negate: bool,
    dir_only: bool,
}

#[derive(Debug, Clone, Default)]
pub struct IgnoreSet {
    rules: Vec<IgnoreRule>,
    /// Always-skipped directory names (regardless of `.gitignore`).
    always_skip: HashSet<String>,
}

impl IgnoreSet {
    pub fn from_root(root: &Path) -> Self {
        let mut rules = Vec::new();
        for name in [".gitignore", ".ignore"] {
            let path = root.join(name);
            let Ok(content) = std::fs::read_to_string(&path) else { continue };
            for raw in content.lines() {
                let line = raw.trim();
                if line.is_empty() || line.starts_with('#') { continue; }
                let mut pattern = line.to_string();
                let negate = pattern.starts_with('!');
                if negate { pattern.remove(0); }
                let dir_only = pattern.ends_with('/');
                if dir_only { pattern.pop(); }
                let anchored = pattern.starts_with('/');
                if anchored { pattern.remove(0); }
                let regex_src = build_ignore_regex(&pattern, anchored);
                if let Ok(re) = regex::Regex::new(&regex_src) {
                    rules.push(IgnoreRule { pattern: re, negate, dir_only });
                }
            }
        }
        // One skip-list, shared with `tool_exec::walk_files` — this used to
        // be a second, independently-maintained set (missing "vendor",
        // present here but absent there) so `grep` and `list_dir` could
        // genuinely disagree about what exists in the repo.
        let always_skip: HashSet<String> =
            super::tool_exec::ALWAYS_SKIP_DIRS.iter().map(|s| s.to_string()).collect();
        Self { rules, always_skip }
    }

    /// `rel` is the path relative to the root (forward-slash separated).
    /// `is_dir` distinguishes trailing-slash `dir/` patterns.
    pub fn is_ignored(&self, rel: &str, is_dir: bool) -> bool {
        // First: always-skip directories.
        if is_dir {
            if let Some(name) = rel.rsplit('/').next() {
                if self.always_skip.contains(name) { return true; }
            }
            if self.always_skip.contains(rel) { return true; }
        }
        let mut ignored = false;
        for r in &self.rules {
            if r.dir_only && !is_dir { continue; }
            if r.pattern.is_match(rel) {
                ignored = !r.negate;
            }
        }
        ignored
    }
}

fn build_ignore_regex(pattern: &str, anchored: bool) -> String {
    const META: &str = r".+()|[]{}^$\";
    let mut out = String::new();
    if anchored { out.push('^'); } else { out.push_str("(^|.*/)"); }
    let mut prev_star = false;
    for c in pattern.chars() {
        match c {
            '*' => {
                if prev_star {
                    // `**` — match across path segments.
                    out.push_str(".*");
                    prev_star = false;
                } else {
                    prev_star = true;
                }
            }
            '?' => {
                if prev_star { out.push_str("[^/]*"); prev_star = false; }
                out.push_str("[^/]");
            }
            c => {
                if prev_star { out.push_str("[^/]*"); prev_star = false; }
                if META.contains(c) { out.push('\\'); }
                out.push(c);
            }
        }
    }
    if prev_star { out.push_str("[^/]*"); }
    out.push_str("($|/.*$)");
    out
}

// ── read_file ────────────────────────────────────────────────────────────────

pub struct ReadFileTool { cwd: String }
impl ReadFileTool { pub fn new(cwd: String) -> Self { Self { cwd } } }

const MAX_READ_LINES: usize = 2000;
const MAX_READ_BYTES: usize = 50_000;

#[async_trait]
impl Tool for ReadFileTool {
    fn name(&self) -> &str { "read_file" }

    async fn execute(&self, args: &serde_json::Value, _cwd: &str) -> ToolResult {
        let path = args["path"].as_str().unwrap_or("").to_string();
        let full = resolve_path(&self.cwd, &path);

        // Existence check.
        let Ok(meta) = tokio::fs::metadata(&full).await else {
            return ToolResult {
                content: format!("Error reading file: {} not found", path),
                is_error: true,
                details: None,
            };
        };
        if meta.is_dir() {
            return ToolResult {
                content: format!("{} is a directory. Use list_dir or ls.", path),
                is_error: true,
                details: None,
            };
        }

        let ext = full.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();

        // ── Jupyter notebook ──
        if ext == "ipynb" {
            let text = match tokio::fs::read_to_string(&full).await {
                Ok(t) => t,
                Err(e) => return ToolResult { content: format!("Error reading notebook: {e}"), is_error: true, details: None },
            };
            let parsed: serde_json::Value = match serde_json::from_str(&text) {
                Ok(v) => v,
                Err(e) => return ToolResult { content: format!("Notebook is not valid JSON: {e}"), is_error: true, details: None },
            };
            return read_notebook_summary(&path, parsed);
        }

        // ── Image ──
        if is_image_ext(&ext) {
            let do_ocr = args.get("ocr").and_then(|v| v.as_bool()).unwrap_or(false);
            if do_ocr {
                return read_image_ocr(&path, &full).await;
            }
            return read_image_summary(&path, &full).await;
        }

        // ── PDF ──
        if ext == "pdf" {
            let pages = args["pages"].as_str().unwrap_or("").to_string();
            return read_pdf_summary(&path, &full, &pages).await;
        }

        // ── Text ──
        let content = match tokio::fs::read_to_string(&full).await {
            Ok(c) => c,
            Err(_) => {
                // Not valid UTF-8: return the size + a hexdump of the first 128 bytes.
                let bytes = match tokio::fs::read(&full).await {
                    Ok(b) => b,
                    Err(e) => return ToolResult { content: format!("Error reading file: {e}"), is_error: true, details: None },
                };
                let head = bytes.iter().take(128).map(|b| format!("{:02x}", b)).collect::<Vec<_>>().join(" ");
                return ToolResult {
                    content: format!("Binary file: {} ({} bytes)\nHead (128 bytes hex):\n{head}", path, bytes.len()),
                    is_error: false,
                    details: Some(json!({ "path": path, "binary": true, "size": bytes.len() })),
                };
            }
        };

        let offset = args["offset"].as_u64().unwrap_or(0) as usize;
        let limit = args["limit"].as_u64().unwrap_or(0) as usize;

        let lines: Vec<&str> = content.lines().collect();
        let start = offset.saturating_sub(1).min(lines.len());
        let end = if limit > 0 { (start + limit).min(lines.len()) } else { lines.len() };
        let mut numbered = String::with_capacity(content.len().min(MAX_READ_BYTES));
        for (i, line) in lines[start..end].iter().enumerate() {
            numbered.push_str(&format!("{:>6}\t{}\n", start + i + 1, line));
        }
        let (mut result, truncated) = truncate_text(&numbered, MAX_READ_LINES, MAX_READ_BYTES);
        // `truncate_text` already appends a notice when it cuts something —
        // but the far more common case is a small file shown in full, where
        // there was previously no signal at all of the total line count.
        // The model had no way to tell "this really is the whole file" from
        // "this got silently cut somewhere I can't see" without this line.
        if !truncated {
            result.push_str(&format!("\n[{} lines total]", lines.len()));
        }
        ToolResult {
            content: result,
            is_error: false,
            details: Some(json!({
                "path": path,
                "lines": lines.len(),
                "shown": end - start,
                "truncated": truncated,
            })),
        }
    }
}

pub(crate) fn is_image_ext(ext: &str) -> bool {
    matches!(ext, "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "tif" | "tiff" | "ico")
}

async fn read_image_summary(display_path: &str, full: &PathBuf) -> ToolResult {
    read_image_summary_impl(display_path.to_string(), full.clone(), false).await
}

async fn read_image_ocr(display_path: &str, full: &PathBuf) -> ToolResult {
    read_image_summary_impl(display_path.to_string(), full.clone(), true).await
}

/// Longest edge a decoded image is downscaled to before being sent to the
/// model — Anthropic's documented ideal max edge for vision input. Without
/// this a multi-megapixel screenshot goes over the wire at full resolution
/// for no accuracy benefit, just token/bandwidth cost.
const MAX_IMAGE_EDGE: u32 = 1568;

async fn read_image_summary_impl(display_path: String, full: PathBuf, do_ocr: bool) -> ToolResult {
    let (w, h, fmt, bytes, image_b64, media_type, encode_err) = {
        let full_path = full.clone();
        let result = tokio::task::spawn_blocking(move || -> Result<(u32, u32, String, u64, Option<String>, &'static str, Option<String>), String> {
            let raw = std::fs::read(&full_path).map_err(|e| e.to_string())?;
            let img = image::load_from_memory(&raw).map_err(|e| e.to_string())?;
            let (w, h) = (img.width(), img.height());
            let format = image::guess_format(&raw).ok();
            let fmt = match format {
                Some(f) => format!("{:?}", f).to_lowercase(),
                None => "unknown".to_string(),
            };

            // Already small enough, and already a format the model can take
            // as-is: pass the original bytes straight through rather than
            // decoding and re-encoding as PNG. For a photo, decode+RGBA8+PNG
            // can ship several MB of lossless base64 where the original JPEG
            // was a fraction of that — a token-cost regression for the sake
            // of a re-encode nothing needed.
            let already_fine = w.max(h) <= MAX_IMAGE_EDGE
                && matches!(format, Some(image::ImageFormat::Png) | Some(image::ImageFormat::Jpeg));
            if already_fine {
                use base64::Engine;
                let media_type = if format == Some(image::ImageFormat::Jpeg) { "image/jpeg" } else { "image/png" };
                let b64 = base64::engine::general_purpose::STANDARD.encode(&raw);
                return Ok((w, h, fmt, raw.len() as u64, Some(b64), media_type, None));
            }

            let longest = w.max(h);
            let scaled = if longest > MAX_IMAGE_EDGE {
                let scale = MAX_IMAGE_EDGE as f64 / longest as f64;
                img.resize((w as f64 * scale) as u32, (h as f64 * scale) as u32, image::imageops::FilterType::Triangle)
            } else {
                img
            };

            // RGB8 unless the source actually has transparency to preserve —
            // an opaque PNG or a resized JPEG doesn't need an all-255 alpha
            // channel inflating what the PNG encoder has to compress.
            let has_alpha = scaled.color().has_alpha();
            let mut png_bytes: Vec<u8> = Vec::with_capacity((scaled.width() * scaled.height()) as usize);
            let encode_err = {
                use image::codecs::png::{CompressionType, FilterType as PngFilterType, PngEncoder};
                use image::ImageEncoder;
                let encoder = PngEncoder::new_with_quality(&mut png_bytes, CompressionType::Fast, PngFilterType::Adaptive);
                let encode_result = if has_alpha {
                    let rgba = scaled.to_rgba8();
                    encoder.write_image(rgba.as_raw(), rgba.width(), rgba.height(), image::ExtendedColorType::Rgba8)
                } else {
                    let rgb = scaled.to_rgb8();
                    encoder.write_image(rgb.as_raw(), rgb.width(), rgb.height(), image::ExtendedColorType::Rgb8)
                };
                encode_result.err().map(|e| e.to_string())
            };
            let image_b64 = if encode_err.is_none() {
                use base64::Engine;
                Some(base64::engine::general_purpose::STANDARD.encode(&png_bytes))
            } else {
                None
            };

            Ok((w, h, fmt, raw.len() as u64, image_b64, "image/png", encode_err))
        }).await;
        match result {
            Ok(Ok(v)) => v,
            Ok(Err(e)) => return ToolResult { content: format!("Error decoding image: {e}"), is_error: true, details: None },
            Err(e) => return ToolResult { content: format!("Internal error: {e}"), is_error: true, details: None },
        }
    };

    let mut content = format!(
        "Image: {}\n  format: {}\n  size: {}×{} px\n  bytes: {}",
        display_path, fmt, w, h, bytes
    );
    if let Some(e) = &encode_err {
        content.push_str(&format!("\n  (could not re-encode this image for viewing: {e} — only this text summary is available)"));
    }

    if do_ocr {
        let ocr_result = super::ocr::ocr_image(&full).await;
        if !ocr_result.text.is_empty() {
            content.push_str(&format!("\n  OCR text ({}):\n{}", ocr_result.method.method_str(), ocr_result.text));
        } else {
            let hint = match ocr_result.method {
                super::ocr::OcrMethod::Unavailable => {
                    "\n  OCR not available: set HUGGINGFACE_TOKEN env var for API inference, or check internet connection."
                }
                _ => "\n  OCR returned no text (image may not contain readable text).",
            };
            content.push_str(hint);
        }
    }

    let mut details = json!({
        "path": display_path,
        "kind": "image",
        "format": fmt,
        "width": w,
        "height": h,
        "bytes": bytes,
    });
    // `image_base64`/`media_type` are the exact keys the tool dispatcher
    // already looks for (see `computer::execute_pointer_action`'s
    // screenshot action) to attach an Image content block — no dispatch-side
    // change needed to wire this up.
    // `media_type` matches whichever path produced `image_b64`: the original
    // file's real type when passed through unmodified, "image/png" whenever
    // this function did the encoding itself.
    if let Some(b64) = image_b64 {
        details["image_base64"] = json!(b64);
        details["media_type"] = json!(media_type);
    }

    ToolResult { content, is_error: false, details: Some(details) }
}

async fn read_pdf_summary(display_path: &str, full: &PathBuf, pages: &str) -> ToolResult {
    let display = display_path.to_string();
    let full = full.clone();
    let pages = pages.to_string();
    let result = tokio::task::spawn_blocking(move || -> Result<(usize, String, u64), String> {
        let bytes = std::fs::read(&full).map_err(|e| e.to_string())?;
        if !bytes.starts_with(b"%PDF-") {
            return Err("not a PDF (missing %PDF- header)".to_string());
        }
        let size = bytes.len() as u64;
        // Cheap page count: `/Type /Page` object occurrences (not /Pages).
        // Regex on the raw bytes; this misses encrypted PDFs but doesn't crash.
        let text = String::from_utf8_lossy(&bytes).to_string();
        let re = regex::Regex::new(r"/Type\s*/Page[^s]").unwrap();
        let page_count = re.find_iter(&text).count();
        // Best-effort text: extract every parenthesized string inside a `BT ... ET`
        // block. This is a real PDF text extractor's toy version — hits the
        // common case of "text in a form-XObject" without decoding CMAP fonts.
        let mut extracted = String::new();
        let bt = regex::Regex::new(r"(?s)BT\b(.*?)\bET").unwrap();
        let tj = regex::Regex::new(r"\((?:\\.|[^\\)])*\)").unwrap();
        for cap in bt.captures_iter(&text) {
            let block = &cap[1];
            for m in tj.find_iter(block) {
                let s = &m.as_str()[1..m.as_str().len() - 1];
                extracted.push_str(&s.replace("\\(", "(").replace("\\)", ")"));
                extracted.push(' ');
            }
            extracted.push('\n');
        }
        let extracted = extracted.trim().to_string();

        // Page filter: keep the requested slice of extracted output when `pages`
        // is set. Since our extractor isn't page-aware, we approximate by
        // splitting on double-newline as a page separator.
        let extracted = if pages.is_empty() {
            extracted
        } else {
            let ranges = parse_page_range(&pages, page_count);
            let all_pages: Vec<&str> = extracted.split("\n\n").collect();
            let mut buf = String::new();
            for (i, p) in all_pages.iter().enumerate() {
                if ranges.contains(&(i + 1)) {
                    buf.push_str(p);
                    buf.push_str("\n\n");
                }
            }
            buf.trim().to_string()
        };
        Ok((page_count, extracted, size))
    }).await;
    match result {
        Ok(Ok((pages_n, text, size))) => {
            let (clipped, truncated) = truncate_text(&text, MAX_READ_LINES, MAX_READ_BYTES);
            let body = if clipped.trim().is_empty() {
                format!("(no text extracted — PDF may be image-only, encrypted, or use custom fonts)")
            } else { clipped };
            ToolResult {
                content: format!(
                    "PDF: {}\n  pages: {pages_n}\n  bytes: {size}\n  extracted text:\n\n{body}",
                    display
                ),
                is_error: false,
                details: Some(json!({
                    "path": display,
                    "kind": "pdf",
                    "pages": pages_n,
                    "bytes": size,
                    "truncated": truncated,
                })),
            }
        }
        Ok(Err(e)) => ToolResult { content: format!("Error reading PDF: {e}"), is_error: true, details: None },
        Err(e) => ToolResult { content: format!("Internal error: {e}"), is_error: true, details: None },
    }
}

fn parse_page_range(spec: &str, total: usize) -> Vec<usize> {
    let mut out = Vec::new();
    for part in spec.split(',') {
        let part = part.trim();
        if let Some((a, b)) = part.split_once('-') {
            let a: usize = a.trim().parse().unwrap_or(1);
            let b: usize = b.trim().parse().unwrap_or(total);
            for i in a..=b.min(total) { out.push(i); }
        } else if let Ok(n) = part.parse::<usize>() {
            out.push(n);
        }
    }
    out
}

fn read_notebook_summary(display_path: &str, nb: serde_json::Value) -> ToolResult {
    let cells = nb.get("cells").and_then(|c| c.as_array()).cloned().unwrap_or_default();
    let mut out = format!("Jupyter notebook: {}\n  cells: {}\n", display_path, cells.len());
    let mut ncode = 0usize;
    let mut nmd = 0usize;
    for (i, cell) in cells.iter().enumerate() {
        let cell_type = cell.get("cell_type").and_then(|v| v.as_str()).unwrap_or("");
        if cell_type == "code" { ncode += 1; } else if cell_type == "markdown" { nmd += 1; }
        let src = cell.get("source");
        let text = match src {
            Some(serde_json::Value::String(s)) => s.clone(),
            Some(serde_json::Value::Array(arr)) => arr.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>().join(""),
            _ => String::new(),
        };
        let head_line = text.lines().next().unwrap_or("").trim();
        let short = if head_line.len() > 100 {
            format!("{}...", &head_line[..100])
        } else {
            head_line.to_string()
        };
        out.push_str(&format!("[{i}] {cell_type}: {short}\n"));
    }
    out.push_str(&format!("\n  code cells: {ncode}, markdown cells: {nmd}\n"));
    ToolResult {
        content: out,
        is_error: false,
        details: Some(json!({
            "path": display_path,
            "kind": "notebook",
            "cells": cells.len(),
            "code_cells": ncode,
            "markdown_cells": nmd,
        })),
    }
}

// ── list_dir ─────────────────────────────────────────────────────────────────

pub struct ListDirTool { cwd: String }
impl ListDirTool { pub fn new(cwd: String) -> Self { Self { cwd } } }

const MAX_TREE_ENTRIES: usize = 1500;
const MAX_TREE_DEPTH: usize = 6;

#[async_trait]
impl Tool for ListDirTool {
    fn name(&self) -> &str { "list_dir" }

    async fn execute(&self, args: &serde_json::Value, _cwd: &str) -> ToolResult {
        let path = args["path"].as_str().unwrap_or(".").to_string();
        let depth = args["depth"].as_u64().unwrap_or(3).min(MAX_TREE_DEPTH as u64) as usize;
        let show_hidden = args["show_hidden"].as_bool().unwrap_or(false);
        let root = resolve_path(&self.cwd, &path);
        let root_for_task = root.clone();

        let result = tokio::task::spawn_blocking(move || -> Result<(String, usize, usize, bool), String> {
            let ignore = IgnoreSet::from_root(&root_for_task);
            let mut out = String::new();
            let mut counted = 0usize;
            let mut total = 0usize;
            let mut truncated = false;
            walk_tree(&root_for_task, &root_for_task, &ignore, 0, depth, show_hidden, &mut out, &mut counted, &mut total, &mut truncated);
            Ok((out, counted, total, truncated))
        }).await;

        match result {
            Ok(Ok((body, counted, total, truncated))) => {
                let mut content = format!(
                    "Listing {} (depth={}, gitignore honored, hidden={}):\n{}",
                    path, depth, show_hidden, body
                );
                if truncated {
                    content.push_str(&format!(
                        "\n[... tree truncated: showing {counted} of {total} entries; \
                         use a narrower path or lower depth ...]\n"
                    ));
                }
                ToolResult {
                    content,
                    is_error: false,
                    details: Some(json!({
                        "path": path,
                        "depth": depth,
                        "entries": counted,
                        "total": total,
                        "truncated": truncated,
                    })),
                }
            }
            Ok(Err(e)) => ToolResult { content: format!("Error listing directory: {e}"), is_error: true, details: None },
            Err(e) => ToolResult { content: format!("Internal error: {e}"), is_error: true, details: None },
        }
    }
}

fn walk_tree(
    root: &Path,
    dir: &Path,
    ignore: &IgnoreSet,
    level: usize,
    max_depth: usize,
    show_hidden: bool,
    out: &mut String,
    counted: &mut usize,
    total: &mut usize,
    truncated: &mut bool,
) {
    if *truncated { return; }
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut items: Vec<(std::path::PathBuf, bool, String)> = Vec::new();
    for e in entries.flatten() {
        let p = e.path();
        let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
        let name = e.file_name().to_string_lossy().to_string();
        if !show_hidden && name.starts_with('.') { continue; }
        let rel = p.strip_prefix(root).unwrap_or(&p).to_string_lossy().replace('\\', "/");
        if ignore.is_ignored(&rel, is_dir) { continue; }
        items.push((p, is_dir, name));
    }
    items.sort_by(|a, b| match (a.1, b.1) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.2.to_ascii_lowercase().cmp(&b.2.to_ascii_lowercase()),
    });
    for (p, is_dir, name) in items {
        *total += 1;
        if *counted >= MAX_TREE_ENTRIES {
            *truncated = true;
            return;
        }
        *counted += 1;
        let indent = "  ".repeat(level);
        if is_dir {
            out.push_str(&format!("{indent}📁 {name}/\n"));
            if level + 1 < max_depth {
                walk_tree(root, &p, ignore, level + 1, max_depth, show_hidden, out, counted, total, truncated);
            }
        } else {
            let bytes = std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
            out.push_str(&format!("{indent}📄 {name} ({} bytes)\n", bytes));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gitignore_matches_directory_pattern() {
        let mut set = IgnoreSet::default();
        set.rules.push(IgnoreRule {
            pattern: regex::Regex::new(&build_ignore_regex("build", false)).unwrap(),
            negate: false,
            dir_only: true,
        });
        assert!(set.is_ignored("build", true));
        assert!(!set.is_ignored("build.rs", false));
    }

    #[test]
    fn parse_pages() {
        assert_eq!(parse_page_range("1-3", 10), vec![1, 2, 3]);
        assert_eq!(parse_page_range("2,4,6-7", 10), vec![2, 4, 6, 7]);
    }
}

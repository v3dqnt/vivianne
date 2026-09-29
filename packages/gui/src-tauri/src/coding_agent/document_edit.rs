//! Editing existing PDF / DOCX / XLSX / PPTX files, and describing their
//! addressable parts so a specific piece can be targeted.
//!
//! `documents.rs` only ever *generated* files. The gap that leaves is not
//! cosmetic: the only way to change one sentence of a report was to regenerate
//! the whole document from a description, which loses every edit made by hand
//! in Word since it was produced. This module changes files in place instead.
//!
//! ## The central design decision: edit the XML, do not regenerate
//!
//! DOCX, XLSX and PPTX are zip archives of XML parts. The tempting
//! implementation is read-everything-into-a-model, mutate, write a fresh file
//! with the generation crates already in `Cargo.toml`. That is far less code
//! and it is quietly destructive: `rust_xlsxwriter` would emit a workbook with
//! the requested cell changed and every style, formula, chart, merged range,
//! pivot table and conditional format silently gone. A tool that damages the
//! user's file while reporting success is worse than one that refuses.
//!
//! So each edit rewrites only the bytes of the one XML part it touches and
//! copies every other entry of the zip through untouched. What this module
//! cannot express, it declines to do.
//!
//! ## Part addressing
//!
//! Every format exposes stable part ids, which is what lets the UI show a list
//! of parts and hand one back to the model:
//!
//! | format | id            | meaning                      |
//! |--------|---------------|------------------------------|
//! | docx   | `p:3`         | 4th paragraph (0-based)      |
//! | xlsx   | `Sheet1!B4`   | one cell                     |
//! | pptx   | `slide:2:1`   | 2nd text box on slide 2      |
//! | pdf    | `page:5`      | one page                     |
//!
//! ## PDF is different, deliberately
//!
//! A PDF has no paragraphs — it has positioned glyph runs, often with subset
//! fonts and no reliable mapping back to characters. Replacing a sentence means
//! re-flowing a layout that was baked at creation time, and every library that
//! claims to do it produces corrupt output on real-world files often enough to
//! be untrustworthy. So PDF editing here is page-level only (delete, rotate,
//! reorder) and text edits are refused with a message that says why and points
//! at regeneration instead. Refusing loudly beats corrupting quietly.

use async_trait::async_trait;
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use super::tool_exec::{Tool, ToolResult};

fn err(msg: impl Into<String>) -> ToolResult {
    ToolResult { content: format!("Error: {}", msg.into()), is_error: true, details: None }
}

fn ok(content: String, details: Value) -> ToolResult {
    ToolResult { content, is_error: false, details: Some(details) }
}

/// One addressable piece of a document.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocPart {
    /// Stable id used by both `edit_document` and the Document panel.
    pub id: String,
    /// What kind of thing this is: paragraph, cell, slideText, page.
    pub kind: String,
    /// Current text. Empty for a part that has none (a blank paragraph).
    pub text: String,
    /// Human label for the UI — "Paragraph 4", "Sheet1!B4", "Slide 2".
    pub label: String,
    /// Whether `edit_document` can actually change this part. A PDF page's
    /// text is listed so it can be read, but is not editable — surfacing that
    /// per-part keeps the UI from offering an action that will be refused.
    pub editable: bool,
}

fn ext_of(path: &Path) -> String {
    path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase()
}

// ─── zip helpers ─────────────────────────────────────────────────────────────

/// Read one entry out of a zip.
fn read_zip_entry(path: &Path, name: &str) -> Result<String, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("couldn't open {}: {e}", path.display()))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| format!("not a valid Office file: {e}"))?;
    let mut entry = zip.by_name(name).map_err(|_| format!("missing '{name}' — file may be corrupt or an unexpected variant"))?;
    let mut s = String::new();
    entry.read_to_string(&mut s).map_err(|e| format!("couldn't read '{name}': {e}"))?;
    Ok(s)
}

/// Names of every entry, so slides can be enumerated without knowing the count.
fn zip_entry_names(path: &Path) -> Result<Vec<String>, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("couldn't open {}: {e}", path.display()))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| format!("not a valid Office file: {e}"))?;
    Ok((0..zip.len()).filter_map(|i| zip.by_index(i).ok().map(|f| f.name().to_string())).collect())
}

/// Rewrite one entry, copying every other byte of the archive through.
///
/// Writes to a temp file and renames, so an interrupted edit leaves the
/// original intact rather than a half-written zip that no application opens.
fn replace_zip_entry(path: &Path, name: &str, new_contents: &str) -> Result<(), String> {
    let file = std::fs::File::open(path).map_err(|e| format!("couldn't open {}: {e}", path.display()))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| format!("not a valid Office file: {e}"))?;

    let tmp: PathBuf = path.with_extension(format!("{}.viv-tmp", ext_of(path)));
    let out = std::fs::File::create(&tmp).map_err(|e| format!("couldn't create temp file: {e}"))?;
    let mut writer = zip::ZipWriter::new(out);

    let mut replaced = false;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|e| format!("corrupt entry: {e}"))?;
        let entry_name = entry.name().to_string();
        // Deflate everything: Office reads either, and preserving each entry's
        // original method would mean threading compression state through for no
        // benefit.
        let opts: zip::write::FileOptions<'_, ()> =
            zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        writer.start_file(&entry_name, opts).map_err(|e| format!("couldn't write entry: {e}"))?;
        if entry_name == name {
            writer.write_all(new_contents.as_bytes()).map_err(|e| format!("couldn't write {name}: {e}"))?;
            replaced = true;
        } else {
            let mut buf = Vec::new();
            entry.read_to_end(&mut buf).map_err(|e| format!("couldn't read entry: {e}"))?;
            writer.write_all(&buf).map_err(|e| format!("couldn't copy entry: {e}"))?;
        }
    }
    writer.finish().map_err(|e| format!("couldn't finalize file: {e}"))?;
    if !replaced {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("'{name}' was not present in the archive"));
    }
    std::fs::rename(&tmp, path).map_err(|e| format!("couldn't replace original: {e}"))?;
    Ok(())
}

// ─── OOXML text walking ──────────────────────────────────────────────────────

/// Collect the text of each container element, in document order.
///
/// `container` is the element that groups one logical block (`w:p` for a Word
/// paragraph, `p:sp` for a PowerPoint shape) and `text_el` is the run-text
/// element inside it (`w:t`, `a:t`). Word splits a single sentence across many
/// runs whenever formatting changes mid-sentence, so the text of a paragraph is
/// the concatenation of its runs, never one element's content.
fn collect_blocks(xml: &str, container: &str, text_el: &str) -> Vec<String> {
    use quick_xml::events::Event;
    let mut reader = quick_xml::Reader::from_str(xml);
    let mut blocks: Vec<String> = Vec::new();
    let mut depth = 0usize;
    let mut in_text = false;
    let mut current = String::new();
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                if name == container {
                    depth += 1;
                    if depth == 1 {
                        current.clear();
                    }
                } else if name == text_el && depth > 0 {
                    in_text = true;
                }
            }
            Ok(Event::Text(t)) if in_text => {
                current.push_str(&t.unescape().unwrap_or_default());
            }
            Ok(Event::End(e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                if name == text_el {
                    in_text = false;
                } else if name == container {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        blocks.push(std::mem::take(&mut current));
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    blocks
}

/// Replace the text of the `index`-th container with `new_text`.
///
/// The replacement goes into the FIRST run of the block and every later run is
/// emptied, rather than being distributed across runs. Distributing would mean
/// guessing which formatting applies to which part of a sentence the user just
/// rewrote; collapsing to the first run means the whole replacement inherits
/// that run's formatting, which is predictable and is what Word itself does
/// when you select a paragraph and type over it.
fn replace_block_text(xml: &str, container: &str, text_el: &str, index: usize, new_text: &str) -> Result<String, String> {
    use quick_xml::events::{BytesText, Event};
    let mut reader = quick_xml::Reader::from_str(xml);
    let mut writer = quick_xml::Writer::new(Vec::new());
    let mut buf = Vec::new();

    let mut depth = 0usize;
    let mut block_idx = 0usize;
    let mut in_target = false;
    let mut in_text = false;
    let mut wrote_replacement = false;
    let mut found = false;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                if name == container {
                    depth += 1;
                    if depth == 1 {
                        in_target = block_idx == index;
                        if in_target {
                            found = true;
                            wrote_replacement = false;
                        }
                    }
                } else if name == text_el && in_target && depth > 0 {
                    in_text = true;
                }
                writer.write_event(Event::Start(e)).map_err(|e| e.to_string())?;
            }
            Ok(Event::Text(t)) => {
                if in_text && in_target {
                    if !wrote_replacement {
                        // `new` (not `new_escaped`) — quick-xml escapes on write,
                        // and pre-escaping would double-encode an `&` into `&amp;amp;`.
                        writer
                            .write_event(Event::Text(BytesText::new(new_text)))
                            .map_err(|e| e.to_string())?;
                        wrote_replacement = true;
                    }
                    // Later runs are dropped entirely (their text is not written).
                } else {
                    writer.write_event(Event::Text(t)).map_err(|e| e.to_string())?;
                }
            }
            Ok(Event::End(e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                if name == text_el {
                    in_text = false;
                }
                writer.write_event(Event::End(e.clone())).map_err(|er| er.to_string())?;
                if name == container {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        if in_target {
                            in_target = false;
                        }
                        block_idx += 1;
                    }
                }
            }
            Ok(Event::Eof) => break,
            Ok(ev) => writer.write_event(ev).map_err(|e| e.to_string())?,
            Err(e) => return Err(format!("malformed XML: {e}")),
        }
        buf.clear();
    }
    if !found {
        return Err(format!("no part at index {index} — the document has {block_idx}"));
    }
    String::from_utf8(writer.into_inner()).map_err(|e| format!("rewrite produced invalid UTF-8: {e}"))
}

// ─── DOCX ────────────────────────────────────────────────────────────────────

const DOCX_BODY: &str = "word/document.xml";

fn docx_parts(path: &Path) -> Result<Vec<DocPart>, String> {
    let xml = read_zip_entry(path, DOCX_BODY)?;
    Ok(collect_blocks(&xml, "w:p", "w:t")
        .into_iter()
        .enumerate()
        .map(|(i, text)| DocPart {
            id: format!("p:{i}"),
            kind: "paragraph".into(),
            label: format!("Paragraph {}", i + 1),
            text,
            editable: true,
        })
        .collect())
}

fn docx_set(path: &Path, index: usize, new_text: &str) -> Result<(), String> {
    let xml = read_zip_entry(path, DOCX_BODY)?;
    let updated = replace_block_text(&xml, "w:p", "w:t", index, new_text)?;
    replace_zip_entry(path, DOCX_BODY, &updated)
}

// ─── PPTX ────────────────────────────────────────────────────────────────────

/// Slide parts, in slide order. Zip entries are not ordered, and `slide10`
/// sorts before `slide2` lexically, so the numeric suffix is parsed out.
fn pptx_slides(path: &Path) -> Result<Vec<(usize, String)>, String> {
    let mut slides: Vec<(usize, String)> = zip_entry_names(path)?
        .into_iter()
        .filter(|n| n.starts_with("ppt/slides/slide") && n.ends_with(".xml"))
        .filter_map(|n| {
            let num: usize = n
                .trim_start_matches("ppt/slides/slide")
                .trim_end_matches(".xml")
                .parse()
                .ok()?;
            Some((num, n))
        })
        .collect();
    slides.sort_by_key(|(n, _)| *n);
    Ok(slides)
}

fn pptx_parts(path: &Path) -> Result<Vec<DocPart>, String> {
    let mut out = Vec::new();
    for (num, entry) in pptx_slides(path)? {
        let xml = read_zip_entry(path, &entry)?;
        for (i, text) in collect_blocks(&xml, "p:sp", "a:t").into_iter().enumerate() {
            out.push(DocPart {
                id: format!("slide:{num}:{i}"),
                kind: "slideText".into(),
                label: format!("Slide {num}, text box {}", i + 1),
                text,
                editable: true,
            });
        }
    }
    Ok(out)
}

fn pptx_set(path: &Path, slide: usize, index: usize, new_text: &str) -> Result<(), String> {
    let entry = format!("ppt/slides/slide{slide}.xml");
    let xml = read_zip_entry(path, &entry)?;
    let updated = replace_block_text(&xml, "p:sp", "a:t", index, new_text)?;
    replace_zip_entry(path, &entry, &updated)
}

// ─── XLSX ────────────────────────────────────────────────────────────────────

/// Sheet name -> worksheet part, in workbook order.
fn xlsx_sheets(path: &Path) -> Result<Vec<(String, String)>, String> {
    let wb = read_zip_entry(path, "xl/workbook.xml")?;
    // Sheet order in workbook.xml is the order shown in Excel; the r:id maps to
    // a worksheet part via the rels file, but the conventional sheetN.xml
    // numbering matches declaration order in every file these tools produce and
    // in everything Excel writes.
    let re = regex::Regex::new(r#"<sheet[^>]*name="([^"]*)""#).map_err(|e| e.to_string())?;
    Ok(re
        .captures_iter(&wb)
        .enumerate()
        .map(|(i, c)| (c[1].to_string(), format!("xl/worksheets/sheet{}.xml", i + 1)))
        .collect())
}

/// Shared strings, which is where most cell text actually lives — a cell with
/// `t="s"` holds an index into this table, not the text itself.
fn xlsx_shared_strings(path: &Path) -> Vec<String> {
    let Ok(xml) = read_zip_entry(path, "xl/sharedStrings.xml") else { return Vec::new() };
    collect_blocks(&xml, "si", "t")
}

fn xlsx_parts(path: &Path) -> Result<Vec<DocPart>, String> {
    let shared = xlsx_shared_strings(path);
    let cell_re = regex::Regex::new(r#"(?s)<c\s+r="([A-Z]+\d+)"([^>]*)>(.*?)</c>"#).map_err(|e| e.to_string())?;
    let v_re = regex::Regex::new(r"(?s)<v>(.*?)</v>").map_err(|e| e.to_string())?;
    let is_re = regex::Regex::new(r"(?s)<is>.*?<t[^>]*>(.*?)</t>.*?</is>").map_err(|e| e.to_string())?;

    let mut out = Vec::new();
    for (name, entry) in xlsx_sheets(path)? {
        let Ok(xml) = read_zip_entry(path, &entry) else { continue };
        for cap in cell_re.captures_iter(&xml) {
            let reference = &cap[1];
            let attrs = &cap[2];
            let body = &cap[3];
            let text = if attrs.contains(r#"t="s""#) {
                v_re.captures(body)
                    .and_then(|v| v[1].parse::<usize>().ok())
                    .and_then(|i| shared.get(i).cloned())
                    .unwrap_or_default()
            } else if attrs.contains(r#"t="inlineStr""#) {
                is_re.captures(body).map(|c| c[1].to_string()).unwrap_or_default()
            } else {
                v_re.captures(body).map(|c| c[1].to_string()).unwrap_or_default()
            };
            if text.is_empty() {
                continue;
            }
            out.push(DocPart {
                id: format!("{name}!{reference}"),
                kind: "cell".into(),
                label: format!("{name}!{reference}"),
                text,
                editable: true,
            });
        }
    }
    Ok(out)
}

/// Set one cell's value, leaving the rest of the workbook byte-identical.
///
/// The cell is rewritten as an inline string rather than being pushed into the
/// shared-string table. Appending to `sharedStrings.xml` means also updating its
/// `count`/`uniqueCount` and risking a stale index elsewhere; an inline string
/// is self-contained, valid OOXML, and Excel opens it without complaint. A
/// numeric-looking value is written as a number so it stays usable in formulas.
fn xlsx_set(path: &Path, sheet_name: &str, reference: &str, new_value: &str) -> Result<(), String> {
    let sheets = xlsx_sheets(path)?;
    let entry = sheets
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(sheet_name))
        .map(|(_, e)| e.clone())
        .ok_or_else(|| {
            format!(
                "no sheet named '{sheet_name}' — this workbook has: {}",
                sheets.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>().join(", ")
            )
        })?;
    let xml = read_zip_entry(path, &entry)?;

    let is_number = new_value.parse::<f64>().is_ok();
    let replacement = if is_number {
        format!(r#"<c r="{reference}"><v>{new_value}</v></c>"#)
    } else {
        let escaped = new_value
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;");
        format!(r#"<c r="{reference}" t="inlineStr"><is><t xml:space="preserve">{escaped}</t></is></c>"#)
    };

    let existing = regex::Regex::new(&format!(r#"(?s)<c\s+r="{reference}"[^>]*(?:/>|>.*?</c>)"#))
        .map_err(|e| e.to_string())?;
    let updated = if existing.is_match(&xml) {
        existing.replace(&xml, replacement.as_str()).to_string()
    } else {
        // Inserting a brand-new cell means finding its row and placing the cell
        // in column order, which OOXML requires. Rather than half-implement
        // that, refuse — the failure mode of an out-of-order cell is a file
        // Excel reports as corrupt.
        return Err(format!(
            "cell {reference} does not exist in '{sheet_name}' yet. This edits existing cells only; \
             use create_document to build a sheet that has it."
        ));
    };
    replace_zip_entry(path, &entry, &updated)
}

// ─── PDF ─────────────────────────────────────────────────────────────────────

fn pdf_parts(path: &Path) -> Result<Vec<DocPart>, String> {
    let doc = lopdf::Document::load(path).map_err(|e| format!("couldn't read PDF: {e}"))?;
    let pages = doc.get_pages();
    Ok(pages
        .keys()
        .map(|num| DocPart {
            id: format!("page:{num}"),
            kind: "page".into(),
            label: format!("Page {num}"),
            text: doc.extract_text(&[*num]).unwrap_or_default().trim().chars().take(400).collect(),
            // Page-level ops only; see this module's header for why text is not
            // editable here.
            editable: false,
        })
        .collect())
}

fn pdf_delete_pages(path: &Path, pages: &[u32]) -> Result<(), String> {
    let mut doc = lopdf::Document::load(path).map_err(|e| format!("couldn't read PDF: {e}"))?;
    doc.delete_pages(pages);
    doc.save(path).map_err(|e| format!("couldn't save PDF: {e}"))?;
    Ok(())
}

// ─── public entry points ─────────────────────────────────────────────────────

/// Every addressable part of a document. Used by `inspect_document` and by the
/// Document panel's part list.
pub fn inspect(path: &Path) -> Result<Vec<DocPart>, String> {
    match ext_of(path).as_str() {
        "docx" => docx_parts(path),
        "xlsx" => xlsx_parts(path),
        "pptx" => pptx_parts(path),
        "pdf" => pdf_parts(path),
        other => Err(format!("can't inspect '.{other}' — supported: pdf, docx, xlsx, pptx")),
    }
}

/// Apply one edit, addressed by part id.
pub fn apply_edit(path: &Path, part: &str, text: &str) -> Result<String, String> {
    match ext_of(path).as_str() {
        "docx" => {
            let idx: usize = part
                .strip_prefix("p:")
                .and_then(|n| n.parse().ok())
                .ok_or("docx part ids look like 'p:3'")?;
            docx_set(path, idx, text)?;
            Ok(format!("Replaced paragraph {} in {}", idx + 1, path.display()))
        }
        "pptx" => {
            let mut it = part.strip_prefix("slide:").ok_or("pptx part ids look like 'slide:2:1'")?.split(':');
            let slide: usize = it.next().and_then(|n| n.parse().ok()).ok_or("pptx part ids look like 'slide:2:1'")?;
            let idx: usize = it.next().and_then(|n| n.parse().ok()).ok_or("pptx part ids look like 'slide:2:1'")?;
            pptx_set(path, slide, idx, text)?;
            Ok(format!("Replaced text box {} on slide {slide} in {}", idx + 1, path.display()))
        }
        "xlsx" => {
            let (sheet, reference) = part.split_once('!').ok_or("xlsx part ids look like 'Sheet1!B4'")?;
            xlsx_set(path, sheet, reference, text)?;
            Ok(format!("Set {part} to '{text}' in {}", path.display()))
        }
        "pdf" => Err(
            "PDF text can't be edited in place: a PDF stores positioned glyphs, not sentences, so \
             replacing text means re-flowing a layout that no longer exists. Page operations \
             (delete_pages) work; for new wording, regenerate with create_document."
                .to_string(),
        ),
        other => Err(format!("can't edit '.{other}' — supported: docx, xlsx, pptx (pdf: pages only)")),
    }
}

// ─── tools ───────────────────────────────────────────────────────────────────

pub struct InspectDocumentTool {
    cwd: String,
}
impl InspectDocumentTool {
    pub fn new(cwd: String) -> Self {
        Self { cwd }
    }
}

fn resolve(cwd: &str, p: &str) -> PathBuf {
    let path = Path::new(p);
    if path.is_absolute() { path.to_path_buf() } else { Path::new(cwd).join(path) }
}

#[async_trait]
impl Tool for InspectDocumentTool {
    fn name(&self) -> &str {
        "inspect_document"
    }
    async fn execute(&self, args: &Value, _cwd: &str) -> ToolResult {
        let Some(rel) = args.get("path").and_then(|v| v.as_str()) else {
            return err("'path' is required");
        };
        let path = resolve(&self.cwd, rel);
        let display = path.display().to_string();
        let parts = match tokio::task::spawn_blocking(move || inspect(&path)).await {
            Ok(Ok(p)) => p,
            Ok(Err(e)) => return err(e),
            Err(e) => return err(format!("inspect task panicked: {e}")),
        };
        if parts.is_empty() {
            return ok(format!("{display} has no addressable text parts."), json!({ "kind": "documentParts", "path": display, "parts": [] }));
        }
        let listing = parts
            .iter()
            .map(|p| {
                let preview: String = p.text.chars().take(120).collect();
                format!("{}  [{}]  {preview}", p.id, if p.editable { "editable" } else { "read-only" })
            })
            .collect::<Vec<_>>()
            .join("\n");
        ok(
            format!("{} parts in {display}:\n{listing}\n\nEdit one with edit_document(path, part, text).", parts.len()),
            json!({ "kind": "documentParts", "path": display, "parts": parts }),
        )
    }
}

pub struct EditDocumentTool {
    cwd: String,
}
impl EditDocumentTool {
    pub fn new(cwd: String) -> Self {
        Self { cwd }
    }
}

#[async_trait]
impl Tool for EditDocumentTool {
    fn name(&self) -> &str {
        "edit_document"
    }
    async fn execute(&self, args: &Value, _cwd: &str) -> ToolResult {
        let Some(rel) = args.get("path").and_then(|v| v.as_str()) else {
            return err("'path' is required");
        };
        let path = resolve(&self.cwd, rel);
        let display = path.display().to_string();
        if !path.exists() {
            return err(format!("{display} doesn't exist — create_document makes a new file; this edits an existing one"));
        }

        // Page deletion is a different shape of operation from a text edit, so
        // it takes its own argument rather than being encoded into `part`.
        if let Some(pages) = args.get("delete_pages").and_then(|v| v.as_array()) {
            let nums: Vec<u32> = pages.iter().filter_map(|v| v.as_u64()).map(|n| n as u32).collect();
            if nums.is_empty() {
                return err("'delete_pages' must be a non-empty array of page numbers");
            }
            let p = path.clone();
            let n = nums.clone();
            return match tokio::task::spawn_blocking(move || pdf_delete_pages(&p, &n)).await {
                Ok(Ok(())) => ok(
                    format!("Deleted page(s) {nums:?} from {display}."),
                    json!({ "kind": "document", "path": display, "format": "pdf" }),
                ),
                Ok(Err(e)) => err(e),
                Err(e) => err(format!("task panicked: {e}")),
            };
        }

        let (Some(part), Some(text)) = (
            args.get("part").and_then(|v| v.as_str()),
            args.get("text").and_then(|v| v.as_str()),
        ) else {
            return err("'part' and 'text' are both required — call inspect_document first to get part ids");
        };
        let (p, part_s, text_s) = (path.clone(), part.to_string(), text.to_string());
        match tokio::task::spawn_blocking(move || apply_edit(&p, &part_s, &text_s)).await {
            Ok(Ok(msg)) => ok(
                msg,
                json!({
                    "kind": "document",
                    "path": display,
                    "format": ext_of(&path),
                    "editedPart": part,
                }),
            ),
            Ok(Err(e)) => err(e),
            Err(e) => err(format!("task panicked: {e}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collects_runs_of_one_paragraph_as_one_block() {
        // Word splits a sentence across runs at every formatting change; a
        // paragraph's text is the concatenation, not any single run.
        let xml = r#"<w:body><w:p><w:r><w:t>Hello </w:t></w:r><w:r><w:t>world</w:t></w:r></w:p>
                     <w:p><w:r><w:t>Second</w:t></w:r></w:p></w:body>"#;
        let blocks = collect_blocks(xml, "w:p", "w:t");
        assert_eq!(blocks, vec!["Hello world".to_string(), "Second".to_string()]);
    }

    #[test]
    fn replacement_collapses_into_the_first_run() {
        let xml = r#"<w:body><w:p><w:r><w:t>Hello </w:t></w:r><w:r><w:t>world</w:t></w:r></w:p></w:body>"#;
        let out = replace_block_text(xml, "w:p", "w:t", 0, "Replaced").expect("rewrite");
        assert_eq!(collect_blocks(&out, "w:p", "w:t"), vec!["Replaced".to_string()]);
        // The second run's element survives (formatting intact), just emptied.
        assert_eq!(out.matches("<w:t>").count(), 2);
    }

    #[test]
    fn only_the_targeted_block_changes() {
        let xml = r#"<w:body><w:p><w:r><w:t>one</w:t></w:r></w:p><w:p><w:r><w:t>two</w:t></w:r></w:p></w:body>"#;
        let out = replace_block_text(xml, "w:p", "w:t", 1, "TWO").expect("rewrite");
        assert_eq!(collect_blocks(&out, "w:p", "w:t"), vec!["one".to_string(), "TWO".to_string()]);
    }

    #[test]
    fn an_out_of_range_index_is_an_error_not_a_silent_no_op() {
        let xml = r#"<w:body><w:p><w:r><w:t>one</w:t></w:r></w:p></w:body>"#;
        assert!(replace_block_text(xml, "w:p", "w:t", 7, "x").is_err());
    }

    #[test]
    fn special_characters_are_escaped_exactly_once() {
        let xml = r#"<w:body><w:p><w:r><w:t>old</w:t></w:r></w:p></w:body>"#;
        let out = replace_block_text(xml, "w:p", "w:t", 0, "A & B <tag>").expect("rewrite");
        assert!(out.contains("&amp;"), "ampersand should be escaped: {out}");
        assert!(!out.contains("&amp;amp;"), "and not double-escaped: {out}");
        assert_eq!(collect_blocks(&out, "w:p", "w:t"), vec!["A & B <tag>".to_string()]);
    }
}

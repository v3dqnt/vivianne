//! Reading documents for the model: PDF, Word, PowerPoint and spreadsheets.
//!
//! `documents.rs` owns editing, and its `inspect` lists addressable parts for
//! `edit_document`. This module answers a different question: what does the
//! document *say*, laid out so a model can follow it? That is:
//!
//! - **PDF:** text page by page (`pdf-extract`, which decodes compressed
//!   streams and font CMaps; lopdf as a fallback). Pages with no usable text,
//!   such as a scan or a paper set in subset fonts with no Unicode map, are
//!   rendered to PNG (`hayro`, pure Rust) so the model can look at them, or
//!   OCR them when it can't see.
//! - **docx:** paragraphs in order, headings as `#`, list items as `-`,
//!   tables as `|` rows, equations' text, then footnotes.
//! - **pptx:** each slide's text and tables, then its speaker notes.
//! - **xlsx/xlsm/xlsb/xls/ods:** each sheet as `|` rows with the values Excel
//!   shows (numbers, dates, cached formula results), capped per sheet.
//!
//! Parsers for untrusted files can panic, so each one runs under
//! `catch_unwind` and a panic is reported as an error, not a crash.

use std::io::Read;
use std::path::Path;

/// Documents this module can read.
pub const READABLE: &[&str] = &["pdf", "docx", "docm", "pptx", "pptm", "xlsx", "xlsm", "xlsb", "xls", "ods"];

/// Old binary Office formats nothing here parses.
pub const LEGACY: &[&str] = &["doc", "ppt", "rtf", "pages", "key", "numbers"];

/// Rows shown per sheet before the rest is summarised.
const MAX_SHEET_ROWS: usize = 400;
/// Columns shown per row.
const MAX_SHEET_COLS: usize = 40;
/// Longest side of a rendered page, in pixels: enough to read a footnote.
const RENDER_MAX_SIDE: f32 = 1600.0;

pub fn ext_of(path: &Path) -> String {
    path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase()
}

fn guarded<T>(what: &str, f: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(r) => r,
        Err(_) => Err(format!("the {what} parser crashed on this file")),
    }
}

// ─── PDF ─────────────────────────────────────────────────────────────────────

/// One page's extracted text, and whether it is worth reading as text.
#[derive(Debug, Clone)]
pub struct PdfPage {
    /// 1-based.
    pub number: usize,
    pub text: String,
    /// True when the text is missing or mostly unmapped glyphs, so the page
    /// has to be looked at (or OCR'd) to be read.
    pub needs_visual: bool,
}

/// Whether extracted text is real text. A scan yields nothing; a font with
/// no Unicode map yields private-use or replacement characters, or control
/// bytes.
fn usable_text(s: &str) -> bool {
    let mut good = 0usize;
    let mut bad = 0usize;
    for c in s.chars() {
        if c.is_whitespace() {
            continue;
        }
        let cp = c as u32;
        if c == '\u{FFFD}' || (0xE000..=0xF8FF).contains(&cp) || (c.is_control()) {
            bad += 1;
        } else {
            good += 1;
        }
    }
    good >= 20 && bad * 5 < good
}

/// Collapse the runs of blank lines and trailing spaces pdf-extract leaves.
fn tidy(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut blank = 0;
    for line in s.lines() {
        let line = line.trim_end();
        if line.trim().is_empty() {
            blank += 1;
            if blank > 1 {
                continue;
            }
        } else {
            blank = 0;
        }
        out.push_str(line);
        out.push('\n');
    }
    out.trim().to_string()
}

pub fn pdf_pages(bytes: &[u8]) -> Result<Vec<PdfPage>, String> {
    if !bytes.starts_with(b"%PDF-") && !bytes[..bytes.len().min(1024)].windows(5).any(|w| w == b"%PDF-") {
        return Err("not a PDF (no %PDF- header)".into());
    }
    let primary = guarded("PDF", || {
        pdf_extract::extract_text_from_mem_by_pages(bytes).map_err(|e| match e {
            pdf_extract::OutputError::PdfError(e) if e.to_string().to_lowercase().contains("encrypt") => {
                "this PDF is password-protected".to_string()
            }
            e => format!("couldn't parse the PDF: {e}"),
        })
    });
    let texts: Vec<String> = match primary {
        Ok(t) if !t.is_empty() => t,
        primary => {
            // lopdf reads some files pdf-extract rejects, with weaker text.
            let fallback = guarded("PDF", || {
                let doc = lopdf::Document::load_mem(bytes).map_err(|e| format!("couldn't parse the PDF: {e}"))?;
                if doc.is_encrypted() {
                    return Err("this PDF is password-protected".into());
                }
                Ok(doc.get_pages().keys().map(|n| doc.extract_text(&[*n]).unwrap_or_default()).collect::<Vec<_>>())
            });
            match (primary, fallback) {
                (_, Ok(t)) if !t.is_empty() => t,
                (Err(e), _) | (_, Err(e)) => return Err(e),
                _ => Vec::new(),
            }
        }
    };
    Ok(texts
        .into_iter()
        .enumerate()
        .map(|(i, t)| {
            let text = tidy(&t);
            PdfPage { number: i + 1, needs_visual: !usable_text(&text), text }
        })
        .collect())
}

/// Page count from the renderer's parser, for files whose text extraction
/// failed outright.
pub fn pdf_page_count(bytes: &[u8]) -> Option<usize> {
    guarded("PDF", || {
        hayro::hayro_syntax::Pdf::new(bytes.to_vec()).map(|p| p.pages().len()).map_err(|e| format!("{e:?}"))
    })
    .ok()
}

/// Render the given 1-based pages to PNG.
pub fn pdf_render(bytes: &[u8], pages: &[usize]) -> Result<Vec<(usize, Vec<u8>)>, String> {
    guarded("PDF renderer", || {
        use hayro::vello_cpu::color::palette::css::WHITE;
        let pdf = hayro::hayro_syntax::Pdf::new(bytes.to_vec()).map_err(|e| format!("couldn't open the PDF to render it: {e:?}"))?;
        let all = pdf.pages();
        let cache = hayro::RenderCache::new();
        let settings = hayro::hayro_interpret::InterpreterSettings::default();
        let mut out = Vec::new();
        for &n in pages {
            let Some(page) = all.get(n.wrapping_sub(1)) else { continue };
            let (w, h) = page.render_dimensions();
            let scale = (RENDER_MAX_SIDE / w.max(h).max(1.0)).min(4.0);
            let pix = hayro::render(
                page,
                &cache,
                &settings,
                &hayro::RenderSettings::default(),
                &hayro::PixmapSettings { x_scale: scale, y_scale: scale, bg_color: WHITE },
            );
            let (pw, ph) = (pix.width() as u32, pix.height() as u32);
            // Opaque white background, so premultiplied equals straight RGBA.
            let rgba = image::RgbaImage::from_raw(pw, ph, pix.data_as_u8_slice().to_vec())
                .ok_or("rendered page had an unexpected size")?;
            let mut png = Vec::new();
            image::DynamicImage::ImageRgba8(rgba)
                .to_rgb8()
                .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
                .map_err(|e| format!("couldn't encode page {n}: {e}"))?;
            out.push((n, png));
        }
        Ok(out)
    })
}

/// "1-3,7" → [1,2,3,7], clamped to `total`. Empty spec → every page.
pub fn parse_range(spec: &str, total: usize) -> Vec<usize> {
    let spec = spec.trim();
    if spec.is_empty() {
        return (1..=total).collect();
    }
    let mut out = Vec::new();
    for part in spec.split(',') {
        let part = part.trim();
        let (a, b) = match part.split_once('-') {
            Some((a, b)) => (a.trim().parse::<usize>().ok(), b.trim().parse::<usize>().ok().or(Some(total))),
            None => {
                let n = part.parse::<usize>().ok();
                (n, n)
            }
        };
        if let (Some(a), Some(b)) = (a, b) {
            for n in a.max(1)..=b.min(total) {
                if !out.contains(&n) {
                    out.push(n);
                }
            }
        }
    }
    out
}

// ─── Office XML ──────────────────────────────────────────────────────────────

fn zip_text(zip: &mut zip::ZipArchive<std::fs::File>, name: &str) -> Option<String> {
    let mut entry = zip.by_name(name).ok()?;
    let mut s = String::new();
    entry.read_to_string(&mut s).ok()?;
    Some(s)
}

fn open_zip(path: &Path) -> Result<zip::ZipArchive<std::fs::File>, String> {
    let f = std::fs::File::open(path).map_err(|e| format!("couldn't open {}: {e}", path.display()))?;
    zip::ZipArchive::new(f).map_err(|e| format!("not a valid Office file: {e}"))
}

fn local(name: &[u8]) -> &[u8] {
    match name.iter().rposition(|b| *b == b':') {
        Some(i) => &name[i + 1..],
        None => name,
    }
}

fn attr(e: &quick_xml::events::BytesStart, key: &[u8]) -> Option<String> {
    e.attributes()
        .flatten()
        .find(|a| local(a.key.as_ref()) == key)
        .and_then(|a| a.unescape_value().ok().map(|v| v.into_owned()))
}

/// Walks WordprocessingML or DrawingML and writes readable text.
///
/// Both share the shape: paragraphs (`p`) of runs with text (`t`), tables
/// (`tbl` > `tr` > `tc`). Word adds paragraph styles and numbering.
struct Walker {
    out: String,
    para: String,
    in_text: bool,
    prefix: &'static str,
    row: Vec<String>,
    cell: String,
    table_depth: usize,
    rows_in_table: usize,
}

impl Walker {
    fn new() -> Self {
        Walker { out: String::new(), para: String::new(), in_text: false, prefix: "", row: Vec::new(), cell: String::new(), table_depth: 0, rows_in_table: 0 }
    }

    fn end_para(&mut self) {
        let text = self.para.trim().to_string();
        self.para.clear();
        if self.table_depth > 0 {
            if !text.is_empty() {
                if !self.cell.is_empty() {
                    self.cell.push(' ');
                }
                self.cell.push_str(&text);
            }
        } else if !text.is_empty() {
            self.out.push_str(self.prefix);
            self.out.push_str(&text);
            self.out.push('\n');
        }
        self.prefix = "";
    }

    fn run(&mut self, xml: &str) {
        use quick_xml::events::Event;
        let mut r = quick_xml::Reader::from_str(xml);
        let mut buf = Vec::new();
        loop {
            match r.read_event_into(&mut buf) {
                Ok(Event::Start(e)) => match local(e.name().as_ref()) {
                    b"t" => self.in_text = true,
                    b"tbl" => {
                        if self.table_depth == 0 {
                            self.end_para();
                            self.rows_in_table = 0;
                        }
                        self.table_depth += 1;
                    }
                    b"tc" => self.cell.clear(),
                    b"numPr" if self.prefix.is_empty() => self.prefix = "- ",
                    _ => {}
                },
                Ok(Event::Empty(e)) => match local(e.name().as_ref()) {
                    b"tab" => self.para.push('\t'),
                    b"br" | b"cr" => self.para.push(' '),
                    b"pStyle" => {
                        let v = attr(&e, b"val").unwrap_or_default().to_lowercase();
                        self.prefix = if v == "title" {
                            "# "
                        } else if let Some(n) = v.strip_prefix("heading").and_then(|n| n.trim().parse::<u8>().ok()) {
                            match n {
                                1 => "# ",
                                2 => "## ",
                                _ => "### ",
                            }
                        } else if v.contains("list") {
                            "- "
                        } else {
                            self.prefix
                        };
                    }
                    b"numPr" | b"buChar" | b"buAutoNum" if self.prefix.is_empty() => self.prefix = "- ",
                    _ => {}
                },
                Ok(Event::Text(t)) if self.in_text => {
                    // quick-xml 0.37 unescapes entities in the text event itself.
                    match t.unescape() {
                        Ok(c) => self.para.push_str(&c),
                        Err(_) => self.para.push_str(&String::from_utf8_lossy(&t)),
                    }
                }
                Ok(Event::End(e)) => match local(e.name().as_ref()) {
                    b"t" => self.in_text = false,
                    b"p" => self.end_para(),
                    b"tc" => {
                        self.end_para();
                        self.row.push(std::mem::take(&mut self.cell).replace('|', "\\|"));
                    }
                    b"tr" => {
                        let cells = std::mem::take(&mut self.row);
                        if self.table_depth == 1 {
                            self.out.push_str(&format!("| {} |\n", cells.join(" | ")));
                            if self.rows_in_table == 0 {
                                self.out.push_str(&format!("|{}\n", " --- |".repeat(cells.len().max(1))));
                            }
                            self.rows_in_table += 1;
                        } else {
                            // A table inside a cell: flatten into the outer cell.
                            self.cell.push_str(&cells.join("; "));
                        }
                    }
                    b"tbl" => {
                        self.table_depth = self.table_depth.saturating_sub(1);
                        if self.table_depth == 0 {
                            self.out.push('\n');
                        }
                    }
                    _ => {}
                },
                Ok(Event::Eof) => break,
                Err(_) => break,
                _ => {}
            }
            buf.clear();
        }
        self.end_para();
    }
}

fn xml_text(xml: &str) -> String {
    let mut w = Walker::new();
    w.run(xml);
    w.out.trim().to_string()
}

pub fn docx_text(path: &Path) -> Result<String, String> {
    guarded("Word", || {
        let mut zip = open_zip(path)?;
        let body = zip_text(&mut zip, "word/document.xml").ok_or("missing word/document.xml: not a Word document")?;
        let mut out = xml_text(&body);
        for (part, title) in [("word/footnotes.xml", "Footnotes"), ("word/endnotes.xml", "Endnotes"), ("word/comments.xml", "Comments")] {
            if let Some(x) = zip_text(&mut zip, part) {
                let t = xml_text(&x);
                if !t.is_empty() {
                    out.push_str(&format!("\n\n## {title}\n{t}"));
                }
            }
        }
        Ok(out)
    })
}

/// Target of the first relationship of `kind` in a `.rels` file, resolved
/// against `base`.
fn rel_target(rels: &str, kind: &str, base: &str) -> Option<String> {
    use quick_xml::events::Event;
    let mut r = quick_xml::Reader::from_str(rels);
    let mut buf = Vec::new();
    loop {
        match r.read_event_into(&mut buf) {
            Ok(Event::Empty(e)) | Ok(Event::Start(e)) if local(e.name().as_ref()) == b"Relationship" => {
                if attr(&e, b"Type").is_some_and(|t| t.ends_with(kind)) {
                    let t = attr(&e, b"Target")?;
                    let mut parts: Vec<&str> = base.split('/').collect();
                    for seg in t.split('/') {
                        match seg {
                            ".." => {
                                parts.pop();
                            }
                            "." | "" => {}
                            s => parts.push(s),
                        }
                    }
                    return Some(parts.join("/"));
                }
            }
            Ok(Event::Eof) | Err(_) => return None,
            _ => {}
        }
        buf.clear();
    }
}

/// Slides in order, each as (number, text). `only` limits to those numbers.
pub fn pptx_slides(path: &Path, only: Option<&[usize]>) -> Result<(usize, Vec<(usize, String)>), String> {
    guarded("PowerPoint", || {
        let mut zip = open_zip(path)?;
        let mut nums: Vec<usize> = zip
            .file_names()
            .filter_map(|n| n.strip_prefix("ppt/slides/slide")?.strip_suffix(".xml")?.parse().ok())
            .collect();
        nums.sort_unstable();
        if nums.is_empty() {
            return Err("no slides found: not a PowerPoint deck".into());
        }
        let total = nums.len();
        let mut out = Vec::new();
        for (i, n) in nums.iter().enumerate() {
            let shown = i + 1;
            if only.is_some_and(|o| !o.contains(&shown)) {
                continue;
            }
            let xml = zip_text(&mut zip, &format!("ppt/slides/slide{n}.xml")).unwrap_or_default();
            let mut text = xml_text(&xml);
            let notes = zip_text(&mut zip, &format!("ppt/slides/_rels/slide{n}.xml.rels"))
                .and_then(|rels| rel_target(&rels, "/notesSlide", "ppt/slides"))
                .and_then(|p| zip_text(&mut zip, &p))
                .map(|x| xml_text(&x))
                .unwrap_or_default();
            // Notes slides repeat the slide number as a lone line; drop it.
            let notes: Vec<&str> = notes.lines().filter(|l| l.trim() != n.to_string() && l.trim() != shown.to_string()).collect();
            if !notes.is_empty() {
                text.push_str(&format!("\nSpeaker notes: {}", notes.join(" ")));
            }
            out.push((shown, text));
        }
        Ok((total, out))
    })
}

// ─── Spreadsheets ────────────────────────────────────────────────────────────

fn cell_str(d: &calamine::Data) -> String {
    use calamine::{Data, DataType};
    match d {
        Data::Empty => String::new(),
        Data::String(s) => s.replace('\n', " ").replace('|', "\\|"),
        Data::Float(f) => {
            if f.fract() == 0.0 && f.abs() < 1e15 {
                format!("{}", *f as i64)
            } else {
                let s = format!("{f:.10}");
                s.trim_end_matches('0').trim_end_matches('.').to_string()
            }
        }
        Data::Int(i) => i.to_string(),
        Data::Bool(b) => if *b { "TRUE" } else { "FALSE" }.into(),
        Data::DateTime(_) | Data::DateTimeIso(_) => match d.as_datetime() {
            Some(dt) if dt.time() == chrono::NaiveTime::MIN => dt.date().to_string(),
            Some(dt) => dt.to_string(),
            None => d.to_string(),
        },
        Data::DurationIso(s) => s.clone(),
        Data::Error(e) => format!("#{e:?}"),
    }
}

/// Every sheet (or the one named) as `|` rows with A1 coordinates.
pub fn sheet_text(path: &Path, only: Option<&str>) -> Result<String, String> {
    guarded("spreadsheet", || {
        use calamine::Reader;
        let mut wb = calamine::open_workbook_auto(path).map_err(|e| format!("couldn't open the spreadsheet: {e}"))?;
        let names = wb.sheet_names().to_vec();
        if let Some(o) = only {
            if !names.iter().any(|n| n.eq_ignore_ascii_case(o)) {
                return Err(format!("no sheet named '{o}'; sheets: {}", names.join(", ")));
            }
        }
        let mut out = format!("Sheets: {}\n", names.join(", "));
        for name in &names {
            if only.is_some_and(|o| !name.eq_ignore_ascii_case(o)) {
                continue;
            }
            let range = match wb.worksheet_range(name) {
                Ok(r) => r,
                Err(e) => {
                    out.push_str(&format!("\n## {name}\n(couldn't read: {e})\n"));
                    continue;
                }
            };
            let Some((r0, c0)) = range.start() else {
                out.push_str(&format!("\n## {name}\n(empty)\n"));
                continue;
            };
            let (h, w) = range.get_size();
            let first_col = col_name(c0 as usize);
            let last_col = col_name(c0 as usize + w.saturating_sub(1));
            out.push_str(&format!("\n## {name} ({first_col}{}:{last_col}{}, {h} rows × {w} columns)\n", r0 + 1, r0 as usize + h));
            let cols = w.min(MAX_SHEET_COLS);
            out.push_str(&format!("| row | {} |\n", (0..cols).map(|c| col_name(c0 as usize + c)).collect::<Vec<_>>().join(" | ")));
            out.push_str(&format!("|{}\n", " --- |".repeat(cols + 1)));
            let mut shown = 0;
            for (i, row) in range.rows().enumerate() {
                if row.iter().all(|c| matches!(c, calamine::Data::Empty)) {
                    continue;
                }
                if shown == MAX_SHEET_ROWS {
                    out.push_str(&format!("… {} more rows not shown (read with sheet=\"{name}\" for this sheet alone, or open it in code)\n", h - i));
                    break;
                }
                let cells: Vec<String> = row.iter().take(cols).map(cell_str).collect();
                out.push_str(&format!("| {} | {} |\n", r0 as usize + i + 1, cells.join(" | ")));
                shown += 1;
            }
            if w > cols {
                out.push_str(&format!("({} more columns not shown)\n", w - cols));
            }
        }
        Ok(out.trim_end().to_string())
    })
}

fn col_name(mut c: usize) -> String {
    let mut s = String::new();
    c += 1;
    while c > 0 {
        let r = (c - 1) % 26;
        s.insert(0, (b'A' + r as u8) as char);
        c = (c - 1) / 26;
    }
    s
}

// ─── One entry point ─────────────────────────────────────────────────────────

/// What to read: page/slide range and sheet.
#[derive(Default, Clone)]
pub struct Want {
    pub pages: String,
    pub sheet: Option<String>,
}

/// A document read for the model.
pub struct Reading {
    pub kind: &'static str,
    /// Pages (PDF), slides (pptx) or 0.
    pub total: usize,
    pub text: String,
    /// PDF pages in the requested range that need to be looked at.
    pub visual_pages: Vec<usize>,
}

pub fn read(path: &Path, want: &Want) -> Result<Reading, String> {
    let ext = ext_of(path);
    match ext.as_str() {
        "pdf" => {
            let bytes = std::fs::read(path).map_err(|e| format!("couldn't read {}: {e}", path.display()))?;
            read_pdf_bytes(&bytes, want)
        }
        "docx" | "docm" => Ok(Reading { kind: "docx", total: 0, text: docx_text(path)?, visual_pages: vec![] }),
        "pptx" | "pptm" => {
            let (total, _) = pptx_slides(path, Some(&[]))?;
            let range = parse_range(&want.pages, total);
            let (_, slides) = pptx_slides(path, Some(&range))?;
            let text = slides.iter().map(|(n, t)| format!("--- Slide {n} ---\n{}", if t.is_empty() { "(no text)" } else { t })).collect::<Vec<_>>().join("\n\n");
            Ok(Reading { kind: "pptx", total, text, visual_pages: vec![] })
        }
        "xlsx" | "xlsm" | "xlsb" | "xls" | "ods" => Ok(Reading { kind: "sheet", total: 0, text: sheet_text(path, want.sheet.as_deref())?, visual_pages: vec![] }),
        e if LEGACY.contains(&e) => Err(format!(
            "'.{e}' is an old binary format this app can't parse. Ask the user to save it as .{} (or PDF), or convert it with LibreOffice: soffice --headless --convert-to {} <file>",
            legacy_target(e),
            legacy_target(e)
        )),
        e => Err(format!("'.{e}' is not a document format this reader handles")),
    }
}

fn legacy_target(e: &str) -> &'static str {
    match e {
        "ppt" | "key" => "pptx",
        "numbers" => "xlsx",
        _ => "docx",
    }
}

pub fn read_pdf_bytes(bytes: &[u8], want: &Want) -> Result<Reading, String> {
    let pages = match pdf_pages(bytes) {
        Ok(p) => p,
        Err(e) if e.contains("password") => return Err(e),
        Err(e) => {
            // Text failed, but the renderer may still open it: every page is visual.
            let n = pdf_page_count(bytes).ok_or(e)?;
            (1..=n).map(|number| PdfPage { number, text: String::new(), needs_visual: true }).collect()
        }
    };
    let total = pages.len();
    let range = parse_range(&want.pages, total);
    let mut text = String::new();
    let mut visual = Vec::new();
    for p in pages.iter().filter(|p| range.contains(&p.number)) {
        text.push_str(&format!("--- Page {} ---\n", p.number));
        if p.needs_visual {
            visual.push(p.number);
            text.push_str(if p.text.trim().is_empty() { "(no text layer: scanned or drawn page)\n\n" } else { "(text layer is unreadable: fonts without a Unicode map)\n\n" });
        } else {
            text.push_str(&p.text);
            text.push_str("\n\n");
        }
    }
    Ok(Reading { kind: "pdf", total, text: text.trim_end().to_string(), visual_pages: visual })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges() {
        assert_eq!(parse_range("", 3), vec![1, 2, 3]);
        assert_eq!(parse_range("2-", 4), vec![2, 3, 4]);
        assert_eq!(parse_range("1-2, 5, 9", 6), vec![1, 2, 5]);
    }

    #[test]
    fn garbage_is_not_text() {
        assert!(!usable_text(""));
        assert!(!usable_text("\u{E001}\u{E002}\u{E003} \u{E004}\u{E005}\u{E006}\u{E007}\u{E008}\u{E009}\u{E00A}\u{E00B}\u{E00C}\u{E00D}\u{E00E}\u{E00F}\u{E010}\u{E011}\u{E012}\u{E013}\u{E014}\u{E015}"));
        assert!(usable_text("Q1. Solve for x: 2x + 3 = 11, showing every step."));
    }

    #[test]
    fn word_xml_keeps_structure() {
        let xml = r#"<w:document xmlns:w="w"><w:body>
          <w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Section A</w:t></w:r></w:p>
          <w:p><w:pPr><w:numPr/></w:pPr><w:r><w:t>first &amp; </w:t></w:r><w:r><w:t xml:space="preserve">item</w:t></w:r></w:p>
          <w:tbl><w:tr><w:tc><w:p><w:r><w:t>Q</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>Marks</w:t></w:r></w:p></w:tc></w:tr>
          <w:tr><w:tc><w:p><w:r><w:t>1</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>5</w:t></w:r></w:p></w:tc></w:tr></w:tbl>
          <w:p><w:r><w:t>After</w:t></w:r></w:p></w:body></w:document>"#;
        let t = xml_text(xml);
        assert!(t.contains("# Section A"), "{t}");
        assert!(t.contains("- first & item"), "{t}");
        assert!(t.contains("| Q | Marks |"), "{t}");
        assert!(t.contains("| 1 | 5 |"), "{t}");
        assert!(t.ends_with("After"), "{t}");
    }

    /// Reads every file in `VIV_DOC_SAMPLES` (`;`-separated) and prints what
    /// the model would get, writing rendered pages next to `VIV_DOC_OUT`.
    /// `cargo test --lib extract::tests::real_files -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn real_files() {
        let list = std::env::var("VIV_DOC_SAMPLES").unwrap_or_default();
        let out = std::path::PathBuf::from(std::env::var("VIV_DOC_OUT").unwrap_or_else(|_| std::env::temp_dir().to_string_lossy().into()));
        for f in list.split(';').filter(|s| !s.is_empty()) {
            let path = std::path::Path::new(f);
            let t = std::time::Instant::now();
            match read(path, &Want::default()) {
                Ok(r) => {
                    let head: String = r.text.chars().take(1500).collect();
                    println!("==== {f}\nkind={} total={} chars={} visual={:?} in {:?}\n{head}\n", r.kind, r.total, r.text.len(), r.visual_pages, t.elapsed());
                    if r.kind == "pdf" {
                        let bytes = std::fs::read(path).unwrap();
                        let pages: Vec<usize> = if r.visual_pages.is_empty() { vec![1] } else { r.visual_pages.iter().copied().take(2).collect() };
                        let t = std::time::Instant::now();
                        match pdf_render(&bytes, &pages) {
                            Ok(shots) => {
                                for (n, png) in shots {
                                    let name = out.join(format!("{}-p{n}.png", path.file_stem().unwrap().to_string_lossy()));
                                    std::fs::write(&name, &png).unwrap();
                                    println!("rendered page {n}: {} bytes -> {} in {:?}", png.len(), name.display(), t.elapsed());
                                }
                            }
                            Err(e) => println!("render failed: {e}"),
                        }
                    }
                }
                Err(e) => println!("==== {f}\nERROR: {e}\n"),
            }
        }
    }

    #[test]
    fn columns() {
        assert_eq!(col_name(0), "A");
        assert_eq!(col_name(25), "Z");
        assert_eq!(col_name(26), "AA");
    }
}

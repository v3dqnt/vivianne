//! Document generation — `create_document`, producing real PDF, DOCX, XLSX
//! and PPTX files the user can open in Word/Excel/PowerPoint.
//!
//! One tool with a `format` rather than four, so the model picks a target
//! rather than a library, and the block vocabulary (`heading` / `paragraph` /
//! `bullets` / `table`) is shared across the prose formats — the same
//! document description renders to PDF or DOCX without rewriting it.
//!
//! Backends, and why:
//!   - **XLSX** — `rust_xlsxwriter`. Mature and complete.
//!   - **DOCX** — `docx-rs`. Solid for the block vocabulary here.
//!   - **PDF** — `printpdf`. Low-level (text is positioned by coordinate),
//!     so line breaking and pagination are done here by hand.
//!   - **PPTX** — hand-written minimal OOXML zipped with `zip`. There is no
//!     credible PowerPoint crate; a .pptx is a zip of XML parts, so the
//!     parts are emitted directly. Deliberately limited to title + bullet
//!     slides: that's what can be produced reliably without a layout engine,
//!     and a half-working deck that PowerPoint refuses to open would be
//!     worse than a plain one it always opens.

use async_trait::async_trait;
use serde_json::Value;

use super::tool_exec::{Tool, ToolResult};

pub struct CreateDocumentTool {
    cwd: String,
}

impl CreateDocumentTool {
    pub fn new(cwd: String) -> Self {
        Self { cwd }
    }
}

fn err(msg: impl Into<String>) -> ToolResult {
    ToolResult { content: format!("Error: {}", msg.into()), is_error: true, details: None }
}

/// One block of prose content, shared by the PDF and DOCX renderers.
enum Block {
    Heading { text: String, level: u8 },
    Paragraph { text: String },
    Bullets { items: Vec<String> },
    Table { rows: Vec<Vec<String>> },
}

fn parse_blocks(v: Option<&Value>) -> Result<Vec<Block>, String> {
    let arr = v.and_then(|b| b.as_array()).ok_or("`blocks` must be an array for pdf/docx")?;
    let mut out = Vec::new();
    for (i, b) in arr.iter().enumerate() {
        let kind = b.get("type").and_then(|t| t.as_str()).unwrap_or("paragraph");
        let text = b.get("text").and_then(|t| t.as_str()).unwrap_or("").to_string();
        match kind {
            "heading" => out.push(Block::Heading {
                text,
                level: b.get("level").and_then(|l| l.as_u64()).unwrap_or(1).clamp(1, 3) as u8,
            }),
            "paragraph" => out.push(Block::Paragraph { text }),
            "bullets" => {
                let items = b
                    .get("items")
                    .and_then(|i| i.as_array())
                    .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
                    .unwrap_or_default();
                out.push(Block::Bullets { items });
            }
            "table" => {
                let rows = b
                    .get("rows")
                    .and_then(|r| r.as_array())
                    .map(|a| {
                        a.iter()
                            .map(|row| {
                                row.as_array()
                                    .map(|cells| cells.iter().map(cell_to_string).collect())
                                    .unwrap_or_default()
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                out.push(Block::Table { rows });
            }
            other => return Err(format!("block {i} has unknown type '{other}' — use heading, paragraph, bullets, or table")),
        }
    }
    Ok(out)
}

/// Cells arrive as strings, numbers, or booleans; everything renders as text
/// except in XLSX, where numbers stay numeric so Excel can sum them.
fn cell_to_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

// ── PDF ───────────────────────────────────────────────────────────────────

const PDF_PAGE_W_MM: f32 = 210.0;
const PDF_PAGE_H_MM: f32 = 297.0;
const PDF_MARGIN_MM: f32 = 20.0;
/// Rough characters-per-line at body size within the text column. printpdf
/// positions glyphs but does no line breaking, so wrapping is done here; a
/// character-count estimate is imprecise for proportional fonts but keeps
/// text inside the margins without measuring every glyph.
const PDF_WRAP_CHARS: usize = 95;

fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    for para in text.split('\n') {
        let mut current = String::new();
        for word in para.split_whitespace() {
            if current.is_empty() {
                current = word.to_string();
            } else if current.len() + 1 + word.len() <= width {
                current.push(' ');
                current.push_str(word);
            } else {
                lines.push(std::mem::take(&mut current));
                current = word.to_string();
            }
        }
        lines.push(current);
    }
    lines
}

fn build_pdf(title: &str, blocks: &[Block]) -> Result<Vec<u8>, String> {
    use printpdf::*;

    let mut doc = PdfDocument::new(title);
    let mut pages: Vec<PdfPage> = Vec::new();
    let mut ops: Vec<Op> = Vec::new();
    let mut y = PDF_PAGE_H_MM - PDF_MARGIN_MM;

    /// Starts a new page once the cursor runs past the bottom margin.
    macro_rules! ensure_space {
        ($needed:expr) => {
            if y - $needed < PDF_MARGIN_MM {
                pages.push(PdfPage::new(Mm(PDF_PAGE_W_MM), Mm(PDF_PAGE_H_MM), std::mem::take(&mut ops)));
                y = PDF_PAGE_H_MM - PDF_MARGIN_MM;
            }
        };
    }

    let write_line = |ops: &mut Vec<Op>, y: f32, text: &str, size: f32, bold: bool| {
        ops.extend_from_slice(&[
            Op::StartTextSection,
            Op::SetTextCursor { pos: Point::new(Mm(PDF_MARGIN_MM), Mm(y)) },
            Op::SetFont {
                font: PdfFontHandle::Builtin(if bold { BuiltinFont::HelveticaBold } else { BuiltinFont::Helvetica }),
                size: Pt(size),
            },
            Op::SetFillColor { col: Color::Rgb(Rgb { r: 0.0, g: 0.0, b: 0.0, icc_profile: None }) },
            Op::ShowText { items: vec![TextItem::Text(text.to_string())] },
            Op::EndTextSection,
        ]);
    };

    if !title.is_empty() {
        ensure_space!(14.0);
        write_line(&mut ops, y, title, 22.0, true);
        y -= 14.0;
    }

    for block in blocks {
        match block {
            Block::Heading { text, level } => {
                let size = match level {
                    1 => 16.0,
                    2 => 13.5,
                    _ => 12.0,
                };
                ensure_space!(10.0);
                y -= 4.0;
                write_line(&mut ops, y, text, size, true);
                y -= 8.0;
            }
            Block::Paragraph { text } => {
                for line in wrap(text, PDF_WRAP_CHARS) {
                    ensure_space!(6.0);
                    write_line(&mut ops, y, &line, 11.0, false);
                    y -= 5.5;
                }
                y -= 3.0;
            }
            Block::Bullets { items } => {
                for item in items {
                    // Continuation lines are indented under the bullet's text,
                    // not under the bullet glyph.
                    for (i, line) in wrap(item, PDF_WRAP_CHARS - 4).into_iter().enumerate() {
                        ensure_space!(6.0);
                        let text = if i == 0 { format!("•  {line}") } else { format!("    {line}") };
                        write_line(&mut ops, y, &text, 11.0, false);
                        y -= 5.5;
                    }
                }
                y -= 3.0;
            }
            Block::Table { rows } => {
                // Fixed-width columns: printpdf has no table primitive, and a
                // monospaced grid is legible without measuring text.
                let widths: Vec<usize> = {
                    let cols = rows.iter().map(|r| r.len()).max().unwrap_or(0);
                    (0..cols)
                        .map(|c| rows.iter().filter_map(|r| r.get(c)).map(|s| s.chars().count()).max().unwrap_or(0).min(28))
                        .collect()
                };
                for (ri, row) in rows.iter().enumerate() {
                    ensure_space!(6.0);
                    let line: Vec<String> = row
                        .iter()
                        .enumerate()
                        .map(|(c, cell)| {
                            let w = widths.get(c).copied().unwrap_or(12);
                            let mut s: String = cell.chars().take(w).collect();
                            while s.chars().count() < w {
                                s.push(' ');
                            }
                            s
                        })
                        .collect();
                    write_line(&mut ops, y, &line.join("  "), 10.0, ri == 0);
                    y -= 5.0;
                }
                y -= 3.0;
            }
        }
    }

    pages.push(PdfPage::new(Mm(PDF_PAGE_W_MM), Mm(PDF_PAGE_H_MM), ops));
    let mut warnings = Vec::new();
    Ok(doc.with_pages(pages).save(&PdfSaveOptions::default(), &mut warnings))
}

// ── DOCX ──────────────────────────────────────────────────────────────────

fn build_docx(path: &std::path::Path, title: &str, blocks: &[Block]) -> Result<(), String> {
    use docx_rs::*;

    let mut docx = Docx::new();

    if !title.is_empty() {
        // Explicit size/bold rather than the "Title" style: applying a named
        // style requires that style to exist in styles.xml, and docx-rs's
        // default document doesn't define one.
        docx = docx.add_paragraph(Paragraph::new().add_run(Run::new().add_text(title).size(44).bold()));
    }

    for block in blocks {
        match block {
            Block::Heading { text, level } => {
                // Word's built-in heading sizes, in half-points.
                let size = match level {
                    1 => 32,
                    2 => 26,
                    _ => 24,
                };
                docx = docx.add_paragraph(Paragraph::new().add_run(Run::new().add_text(text).size(size).bold()));
            }
            Block::Paragraph { text } => {
                docx = docx.add_paragraph(Paragraph::new().add_run(Run::new().add_text(text)));
            }
            Block::Bullets { items } => {
                for item in items {
                    // A literal bullet rather than a numbering definition:
                    // real list numbering needs a numbering.xml part, and a
                    // wrong one makes Word repair the file on open.
                    docx = docx.add_paragraph(Paragraph::new().add_run(Run::new().add_text(format!("• {item}"))));
                }
            }
            Block::Table { rows } => {
                let table_rows: Vec<TableRow> = rows
                    .iter()
                    .map(|row| {
                        TableRow::new(
                            row.iter()
                                .map(|cell| {
                                    TableCell::new()
                                        .add_paragraph(Paragraph::new().add_run(Run::new().add_text(cell)))
                                })
                                .collect(),
                        )
                    })
                    .collect();
                if !table_rows.is_empty() {
                    docx = docx.add_table(Table::new(table_rows));
                }
            }
        }
    }

    let file = std::fs::File::create(path).map_err(|e| format!("couldn't create file: {e}"))?;
    docx.build().pack(file).map_err(|e| format!("docx write failed: {e}"))?;
    Ok(())
}

// ── XLSX ──────────────────────────────────────────────────────────────────

fn build_xlsx(path: &std::path::Path, sheets: &Value) -> Result<(), String> {
    use rust_xlsxwriter::{Format, Workbook};

    let sheets = sheets.as_array().ok_or("`sheets` must be an array for xlsx")?;
    if sheets.is_empty() {
        return Err("`sheets` was empty — an xlsx needs at least one sheet".to_string());
    }

    let mut workbook = Workbook::new();
    let header = Format::new().set_bold();

    for (si, sheet) in sheets.iter().enumerate() {
        let worksheet = workbook.add_worksheet();
        if let Some(name) = sheet.get("name").and_then(|n| n.as_str()) {
            // Excel rejects >31 chars and a handful of characters outright;
            // a rejected name would fail the whole save.
            let safe: String = name.chars().filter(|c| !"[]:*?/\\".contains(*c)).take(31).collect();
            if !safe.is_empty() {
                worksheet.set_name(&safe).map_err(|e| format!("sheet {si} name rejected: {e}"))?;
            }
        }
        let rows = sheet.get("rows").and_then(|r| r.as_array()).ok_or(format!("sheet {si} has no `rows` array"))?;
        for (r, row) in rows.iter().enumerate() {
            let Some(cells) = row.as_array() else { continue };
            for (c, cell) in cells.iter().enumerate() {
                let (row_i, col_i) = (r as u32, c as u16);
                // Numbers stay numeric so Excel can actually compute with
                // them; writing everything as text is the classic way to
                // produce a spreadsheet nobody can sum.
                let res = match cell {
                    Value::Number(n) => n.as_f64().map(|f| worksheet.write_number(row_i, col_i, f)),
                    Value::Bool(b) => Some(worksheet.write_boolean(row_i, col_i, *b)),
                    Value::Null => None,
                    other => {
                        let s = cell_to_string(other);
                        Some(if r == 0 {
                            worksheet.write_string_with_format(row_i, col_i, &s, &header)
                        } else {
                            worksheet.write_string(row_i, col_i, &s)
                        })
                    }
                };
                if let Some(Err(e)) = res {
                    return Err(format!("sheet {si} cell r{r}c{c}: {e}"));
                }
            }
        }
    }

    workbook.save(path).map_err(|e| format!("xlsx write failed: {e}"))?;
    Ok(())
}

// ── PPTX ──────────────────────────────────────────────────────────────────

/// XML-escape. Every string below is interpolated into markup, so an
/// unescaped `&` or `<` in user text produces a file PowerPoint refuses.
fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&apos;")
}

/// A single title-and-bullets slide's XML.
fn pptx_slide_xml(title: &str, bullets: &[String]) -> String {
    let body: String = if bullets.is_empty() {
        // An empty body placeholder still needs one paragraph or PowerPoint
        // treats the shape as malformed.
        "<a:p/>".to_string()
    } else {
        bullets
            .iter()
            .map(|b| format!("<a:p><a:r><a:rPr lang=\"en-US\" dirty=\"0\"/><a:t>{}</a:t></a:r></a:p>", xml_escape(b)))
            .collect()
    };

    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:sld xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main">
<p:cSld><p:spTree>
<p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr>
<p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/><a:chOff x="0" y="0"/><a:chExt cx="0" cy="0"/></a:xfrm></p:grpSpPr>
<p:sp>
<p:nvSpPr><p:cNvPr id="2" name="Title"/><p:cNvSpPr><a:spLocks noGrp="1"/></p:cNvSpPr><p:nvPr/></p:nvSpPr>
<p:spPr><a:xfrm><a:off x="628650" y="365126"/><a:ext cx="7886700" cy="1325563"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></p:spPr>
<p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:r><a:rPr lang="en-US" sz="3200" b="1" dirty="0"/><a:t>{title}</a:t></a:r></a:p></p:txBody>
</p:sp>
<p:sp>
<p:nvSpPr><p:cNvPr id="3" name="Content"/><p:cNvSpPr><a:spLocks noGrp="1"/></p:cNvSpPr><p:nvPr/></p:nvSpPr>
<p:spPr><a:xfrm><a:off x="628650" y="1825625"/><a:ext cx="7886700" cy="4351338"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></p:spPr>
<p:txBody><a:bodyPr/><a:lstStyle/>{body}</p:txBody>
</p:sp>
</p:spTree></p:cSld><p:clrMapOvr><a:overrideClrMapping bg1="lt1" tx1="dk1" bg2="lt2" tx2="dk2" accent1="accent1" accent2="accent2" accent3="accent3" accent4="accent4" accent5="accent5" accent6="accent6" hlink="hlink" folHlink="folHlink"/></p:clrMapOvr>
</p:sld>"#,
        title = xml_escape(title),
        body = body
    )
}

fn build_pptx(path: &std::path::Path, slides: &Value) -> Result<(), String> {
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    let slides = slides.as_array().ok_or("`slides` must be an array for pptx")?;
    if slides.is_empty() {
        return Err("`slides` was empty — a pptx needs at least one slide".to_string());
    }

    let parsed: Vec<(String, Vec<String>)> = slides
        .iter()
        .map(|s| {
            let title = s.get("title").and_then(|t| t.as_str()).unwrap_or("").to_string();
            let bullets = s
                .get("bullets")
                .and_then(|b| b.as_array())
                .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
                .unwrap_or_default();
            (title, bullets)
        })
        .collect();

    let file = std::fs::File::create(path).map_err(|e| format!("couldn't create file: {e}"))?;
    let mut zip = zip::ZipWriter::new(file);
    let opts = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    let mut add = |name: &str, data: &str| -> Result<(), String> {
        zip.start_file(name, opts).map_err(|e| format!("zip {name}: {e}"))?;
        zip.write_all(data.as_bytes()).map_err(|e| format!("zip write {name}: {e}"))?;
        Ok(())
    };

    // [Content_Types].xml — every part must be declared or PowerPoint
    // reports the file as corrupt before reading a single slide.
    let slide_overrides: String = (1..=parsed.len())
        .map(|i| format!(r#"<Override PartName="/ppt/slides/slide{i}.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slide+xml"/>"#))
        .collect();
    add(
        "[Content_Types].xml",
        &format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
<Default Extension="xml" ContentType="application/xml"/>
<Override PartName="/ppt/presentation.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml"/>
<Override PartName="/ppt/slideMasters/slideMaster1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slideMaster+xml"/>
<Override PartName="/ppt/slideLayouts/slideLayout1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slideLayout+xml"/>
<Override PartName="/ppt/theme/theme1.xml" ContentType="application/vnd.openxmlformats-officedocument.theme+xml"/>
{slide_overrides}
</Types>"#
        ),
    )?;

    add(
        "_rels/.rels",
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="ppt/presentation.xml"/>
</Relationships>"#,
    )?;

    // Slide ids start at 256: PowerPoint reserves lower values.
    let sld_id_list: String = (1..=parsed.len())
        .map(|i| format!(r#"<p:sldId id="{}" r:id="rId{}"/>"#, 255 + i, i + 1))
        .collect();
    add(
        "ppt/presentation.xml",
        &format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:presentation xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main">
<p:sldMasterIdLst><p:sldMasterId id="2147483648" r:id="rId1"/></p:sldMasterIdLst>
<p:sldIdLst>{sld_id_list}</p:sldIdLst>
<p:sldSz cx="9144000" cy="6858000"/><p:notesSz cx="6858000" cy="9144000"/>
</p:presentation>"#
        ),
    )?;

    let pres_rels: String = (1..=parsed.len())
        .map(|i| format!(r#"<Relationship Id="rId{}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide{i}.xml"/>"#, i + 1))
        .collect();
    add(
        "ppt/_rels/presentation.xml.rels",
        &format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster" Target="slideMasters/slideMaster1.xml"/>
{pres_rels}
</Relationships>"#
        ),
    )?;

    add("ppt/theme/theme1.xml", THEME_XML)?;
    add("ppt/slideMasters/slideMaster1.xml", SLIDE_MASTER_XML)?;
    add(
        "ppt/slideMasters/_rels/slideMaster1.xml.rels",
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout" Target="../slideLayouts/slideLayout1.xml"/>
<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme" Target="../theme/theme1.xml"/>
</Relationships>"#,
    )?;
    add("ppt/slideLayouts/slideLayout1.xml", SLIDE_LAYOUT_XML)?;
    add(
        "ppt/slideLayouts/_rels/slideLayout1.xml.rels",
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster" Target="../slideMasters/slideMaster1.xml"/>
</Relationships>"#,
    )?;

    for (i, (title, bullets)) in parsed.iter().enumerate() {
        add(&format!("ppt/slides/slide{}.xml", i + 1), &pptx_slide_xml(title, bullets))?;
        add(
            &format!("ppt/slides/_rels/slide{}.xml.rels", i + 1),
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout" Target="../slideLayouts/slideLayout1.xml"/>
</Relationships>"#,
        )?;
    }

    zip.finish().map_err(|e| format!("zip finish: {e}"))?;
    Ok(())
}

const THEME_XML: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" name="Office">
<a:themeElements>
<a:clrScheme name="Office"><a:dk1><a:sysClr val="windowText" lastClr="000000"/></a:dk1><a:lt1><a:sysClr val="window" lastClr="FFFFFF"/></a:lt1><a:dk2><a:srgbClr val="44546A"/></a:dk2><a:lt2><a:srgbClr val="E7E6E6"/></a:lt2><a:accent1><a:srgbClr val="4472C4"/></a:accent1><a:accent2><a:srgbClr val="ED7D31"/></a:accent2><a:accent3><a:srgbClr val="A5A5A5"/></a:accent3><a:accent4><a:srgbClr val="FFC000"/></a:accent4><a:accent5><a:srgbClr val="5B9BD5"/></a:accent5><a:accent6><a:srgbClr val="70AD47"/></a:accent6><a:hlink><a:srgbClr val="0563C1"/></a:hlink><a:folHlink><a:srgbClr val="954F72"/></a:folHlink></a:clrScheme>
<a:fontScheme name="Office"><a:majorFont><a:latin typeface="Calibri Light"/><a:ea typeface=""/><a:cs typeface=""/></a:majorFont><a:minorFont><a:latin typeface="Calibri"/><a:ea typeface=""/><a:cs typeface=""/></a:minorFont></a:fontScheme>
<a:fmtScheme name="Office">
<a:fillStyleLst><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:fillStyleLst>
<a:lnStyleLst><a:ln w="6350"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln><a:ln w="12700"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln><a:ln w="19050"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln></a:lnStyleLst>
<a:effectStyleLst><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/></a:effectStyle></a:effectStyleLst>
<a:bgFillStyleLst><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:bgFillStyleLst>
</a:fmtScheme>
</a:themeElements>
</a:theme>"#;

const SLIDE_MASTER_XML: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:sldMaster xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main">
<p:cSld><p:spTree>
<p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr>
<p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/><a:chOff x="0" y="0"/><a:chExt cx="0" cy="0"/></a:xfrm></p:grpSpPr>
</p:spTree></p:cSld>
<p:clrMap bg1="lt1" tx1="dk1" bg2="lt2" tx2="dk2" accent1="accent1" accent2="accent2" accent3="accent3" accent4="accent4" accent5="accent5" accent6="accent6" hlink="hlink" folHlink="folHlink"/>
<p:sldLayoutIdLst><p:sldLayoutId id="2147483649" r:id="rId1"/></p:sldLayoutIdLst>
</p:sldMaster>"#;

const SLIDE_LAYOUT_XML: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:sldLayout xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" type="titleAndBody" preserve="1">
<p:cSld name="Title and Content"><p:spTree>
<p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr>
<p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/><a:chOff x="0" y="0"/><a:chExt cx="0" cy="0"/></a:xfrm></p:grpSpPr>
</p:spTree></p:cSld>
<p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr>
</p:sldLayout>"#;

// ── Tool ──────────────────────────────────────────────────────────────────

#[async_trait]
impl Tool for CreateDocumentTool {
    fn name(&self) -> &str {
        "create_document"
    }

    async fn execute(&self, args: &Value, _cwd: &str) -> ToolResult {
        let Some(format) = args.get("format").and_then(|f| f.as_str()) else {
            return err("'format' is required — pdf, docx, xlsx, or pptx");
        };
        let Some(rel_path) = args.get("path").and_then(|p| p.as_str()).filter(|p| !p.trim().is_empty()) else {
            return err("'path' is required — where to write the document");
        };

        let base = std::path::Path::new(&self.cwd);
        let path = {
            let p = std::path::Path::new(rel_path);
            if p.is_absolute() { p.to_path_buf() } else { base.join(p) }
        };
        if let Some(parent) = path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                return err(format!("couldn't create {}: {e}", parent.display()));
            }
        }

        let title = args.get("title").and_then(|t| t.as_str()).unwrap_or("").to_string();
        let args = args.clone();
        let format = format.to_lowercase();
        let path_for_task = path.clone();

        // Every backend is synchronous and CPU-bound (zip deflate, PDF
        // serialization), so none of it belongs on an async worker.
        let result = tokio::task::spawn_blocking(move || -> Result<u64, String> {
            match format.as_str() {
                "pdf" => {
                    let blocks = parse_blocks(args.get("blocks"))?;
                    let bytes = build_pdf(&title, &blocks)?;
                    std::fs::write(&path_for_task, &bytes).map_err(|e| format!("couldn't write pdf: {e}"))?;
                }
                "docx" => {
                    let blocks = parse_blocks(args.get("blocks"))?;
                    build_docx(&path_for_task, &title, &blocks)?;
                }
                "xlsx" => {
                    let sheets = args.get("sheets").ok_or("`sheets` is required for xlsx")?;
                    build_xlsx(&path_for_task, sheets)?;
                }
                "pptx" => {
                    let slides = args.get("slides").ok_or("`slides` is required for pptx")?;
                    build_pptx(&path_for_task, slides)?;
                }
                other => return Err(format!("unknown format '{other}' — use pdf, docx, xlsx, or pptx")),
            }
            std::fs::metadata(&path_for_task).map(|m| m.len()).map_err(|e| format!("couldn't stat written file: {e}"))
        })
        .await;

        let display = path.display().to_string();
        match result {
            Ok(Ok(bytes)) => ToolResult {
                content: format!("Created {display} ({bytes} bytes)."),
                is_error: false,
                // `kind: "document"` is what the chat's tool card keys off to
                // render a file card (with an inline preview for PDFs)
                // instead of a line of text.
                details: Some(serde_json::json!({
                    "kind": "document",
                    "path": display,
                    "format": path.extension().and_then(|e| e.to_str()).unwrap_or(""),
                    "bytes": bytes,
                })),
            },
            Ok(Err(e)) => err(e),
            Err(e) => err(format!("document task panicked: {e}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xml_escape_covers_the_five_entities() {
        assert_eq!(xml_escape(r#"a&b<c>d"e'f"#), "a&amp;b&lt;c&gt;d&quot;e&apos;f");
    }

    /// An unescaped `&` in a slide title is the classic way to produce a
    /// .pptx PowerPoint refuses to open.
    #[test]
    fn slide_xml_escapes_user_text() {
        let xml = pptx_slide_xml("Q1 & Q2", &["a < b".to_string()]);
        assert!(xml.contains("Q1 &amp; Q2"));
        assert!(xml.contains("a &lt; b"));
        assert!(!xml.contains("Q1 & Q2"));
    }

    #[test]
    fn wrap_breaks_on_width_and_keeps_all_words() {
        let lines = wrap("one two three four five", 9);
        assert!(lines.iter().all(|l| l.len() <= 9), "over-long line: {lines:?}");
        assert_eq!(lines.join(" "), "one two three four five");
    }

    #[test]
    fn empty_bullets_still_emit_a_paragraph() {
        // PowerPoint treats a body placeholder with no <a:p> as malformed.
        assert!(pptx_slide_xml("T", &[]).contains("<a:p/>"));
    }
}

#[cfg(test)]
mod roundtrip {
    use super::*;

    /// Writes one of each format to a temp dir. Not a unit test of layout —
    /// the point is that every backend produces a file the corresponding app
    /// will actually open, which is what a wrong OOXML part or a truncated
    /// PDF breaks.
    #[test]
    #[ignore = "run manually: cargo test -p app --lib roundtrip -- --ignored --nocapture"]
    fn writes_all_four_formats() {
        let dir = std::env::temp_dir().join("viv_doc_roundtrip");
        std::fs::create_dir_all(&dir).unwrap();
        let blocks = vec![
            Block::Heading { text: "Overview".into(), level: 1 },
            Block::Paragraph { text: "A paragraph long enough to need wrapping across more than a single rendered line in the PDF output.".into() },
            Block::Bullets { items: vec!["First & foremost".into(), "Second <point>".into()] },
            Block::Table { rows: vec![vec!["Name".into(), "Qty".into()], vec!["Widget".into(), "3".into()]] },
        ];
        std::fs::write(dir.join("out.pdf"), build_pdf("Test Report", &blocks).unwrap()).unwrap();
        build_docx(&dir.join("out.docx"), "Test Report", &blocks).unwrap();
        build_xlsx(&dir.join("out.xlsx"), &serde_json::json!([
            { "name": "Sheet1", "rows": [["Item", "Count"], ["Widget", 3], ["Gadget", 4.5]] }
        ])).unwrap();
        build_pptx(&dir.join("out.pptx"), &serde_json::json!([
            { "title": "Q1 & Q2", "bullets": ["Revenue up", "Costs <down>"] },
            { "title": "Next", "bullets": [] }
        ])).unwrap();
        println!("wrote to {}", dir.display());
    }
}

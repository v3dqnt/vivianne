//! `create_document`: real PDF, Word, Excel and PowerPoint files, designed.
//!
//! One tool with a `format`, one content vocabulary (`doc_design::Block`) and
//! one set of themes for every format, so the model describes a document
//! once and it looks like the same document whatever it's saved as.
//!
//! Backends, and why:
//!   - **PDF** — HTML/CSS printed by the system's Chromium (`doc_design`):
//!     real typography, every script, tables, images, SVG charts, running
//!     page numbers. The model can also pass its own `html` for full control,
//!     or `slides` for a 16:9 PDF deck. Without a browser it falls back to
//!     `printpdf` (plain, Latin-only), and says so.
//!   - **DOCX** — `docx-rs`, with real styles (so Word's navigation pane and
//!     table of contents work), real list numbering, styled tables, images,
//!     and charts as images. Stays fully editable.
//!   - **XLSX** — `rust_xlsxwriter`: themed header, number formats, formulas,
//!     totals, frozen header, filters, and native Excel charts that stay live.
//!   - **PPTX** — OOXML written here (there is no credible PowerPoint crate):
//!     16:9, themed, with title, section, bullets, two-column, image, quote,
//!     stats, table and chart layouts. Text stays editable; charts are images.
//!
//! Every format can return a preview image of its first page or slide, so the
//! model can look at what it made and fix it before telling the user.

use std::path::Path;

use async_trait::async_trait;
use serde_json::Value;

use super::doc_design::{self as dd, Block, Chart, DocMeta, Image, Slide, Theme};
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

// ── PDF fallback (no browser) ────────────────────────────────────────────

const PDF_PAGE_W_MM: f32 = 210.0;
const PDF_PAGE_H_MM: f32 = 297.0;
const PDF_MARGIN_MM: f32 = 20.0;
/// Rough characters-per-line at body size; printpdf does no line breaking.
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

/// Plain text, built-in Helvetica: only used when no browser is installed.
fn build_pdf_plain(title: &str, blocks: &[Block]) -> Result<Vec<u8>, String> {
    use printpdf::*;

    let mut doc = PdfDocument::new(title);
    let mut pages: Vec<PdfPage> = Vec::new();
    let mut ops: Vec<Op> = Vec::new();
    let mut y = PDF_PAGE_H_MM - PDF_MARGIN_MM;

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
            Op::SetFont { font: PdfFontHandle::Builtin(if bold { BuiltinFont::HelveticaBold } else { BuiltinFont::Helvetica }), size: Pt(size) },
            Op::SetFillColor { col: Color::Rgb(Rgb { r: 0.0, g: 0.0, b: 0.0, icc_profile: None }) },
            Op::ShowText { items: vec![TextItem::Text(text.to_string())] },
            Op::EndTextSection,
        ]);
    };
    let para = |pages: &mut Vec<PdfPage>, ops: &mut Vec<Op>, y: &mut f32, text: &str, size: f32, bold: bool, indent: &str| {
        for (i, line) in wrap(text, PDF_WRAP_CHARS - indent.len()).into_iter().enumerate() {
            if *y - 6.0 < PDF_MARGIN_MM {
                pages.push(PdfPage::new(Mm(PDF_PAGE_W_MM), Mm(PDF_PAGE_H_MM), std::mem::take(ops)));
                *y = PDF_PAGE_H_MM - PDF_MARGIN_MM;
            }
            let lead = if i == 0 { indent.to_string() } else { " ".repeat(indent.chars().count()) };
            write_line(ops, *y, &format!("{lead}{line}"), size, bold);
            *y -= size * 0.5;
        }
        *y -= 3.0;
    };

    if !title.is_empty() {
        ensure_space!(14.0);
        write_line(&mut ops, y, title, 22.0, true);
        y -= 14.0;
    }
    for block in blocks {
        match block {
            Block::Heading { text, level } => {
                y -= 4.0;
                para(&mut pages, &mut ops, &mut y, &dd::plain(text), match level { 1 => 16.0, 2 => 13.5, _ => 12.0 }, true, "");
            }
            Block::Paragraph { text } => para(&mut pages, &mut ops, &mut y, &dd::plain(text), 11.0, false, ""),
            Block::List { items, ordered } => {
                for (i, (t, l)) in items.iter().enumerate() {
                    let pad = "    ".repeat(*l as usize);
                    let mark = if *ordered { format!("{pad}{}. ", i + 1) } else { format!("{pad}•  ") };
                    para(&mut pages, &mut ops, &mut y, &dd::plain(t), 11.0, false, &mark);
                }
            }
            Block::Table { rows, .. } => {
                for row in rows {
                    para(&mut pages, &mut ops, &mut y, &row.iter().map(|c| dd::plain(c)).collect::<Vec<_>>().join("  |  "), 10.0, false, "");
                }
            }
            Block::Quote { text, by } => para(&mut pages, &mut ops, &mut y, &format!("\"{}\" {}", dd::plain(text), by), 11.0, false, "    "),
            Block::Callout { title, text, .. } => para(&mut pages, &mut ops, &mut y, &format!("{} {}", dd::plain(title), dd::plain(text)), 11.0, false, "  > "),
            Block::Code { text, .. } => para(&mut pages, &mut ops, &mut y, text, 9.5, false, "    "),
            Block::Metrics { items } => {
                for m in items {
                    para(&mut pages, &mut ops, &mut y, &format!("{}: {} {}", m.label, m.value, m.note), 11.0, true, "");
                }
            }
            Block::Chart { chart } => {
                para(&mut pages, &mut ops, &mut y, &chart.title, 11.0, true, "");
                for (i, l) in chart.labels.iter().enumerate() {
                    let vals: Vec<String> = chart.series.iter().map(|s| s.values.get(i).map(|v| v.to_string()).unwrap_or_default()).collect();
                    para(&mut pages, &mut ops, &mut y, &format!("{l}: {}", vals.join(", ")), 10.0, false, "  ");
                }
            }
            Block::Image { caption, .. } => para(&mut pages, &mut ops, &mut y, &format!("[image] {caption}"), 10.0, false, ""),
            Block::Divider => y -= 6.0,
            Block::PageBreak => {
                pages.push(PdfPage::new(Mm(PDF_PAGE_W_MM), Mm(PDF_PAGE_H_MM), std::mem::take(&mut ops)));
                y = PDF_PAGE_H_MM - PDF_MARGIN_MM;
            }
        }
    }
    pages.push(PdfPage::new(Mm(PDF_PAGE_W_MM), Mm(PDF_PAGE_H_MM), ops));
    let mut warnings = Vec::new();
    Ok(doc.with_pages(pages).save(&PdfSaveOptions::default(), &mut warnings))
}

// ── DOCX ──────────────────────────────────────────────────────────────────

/// Twips (1/20 pt) per millimetre, and EMU per millimetre.
const TWIP_MM: f32 = 56.692;
const EMU_MM: u32 = 36_000;

fn docx_runs(text: &str, size: Option<usize>, color: Option<&str>, theme: &Theme) -> Vec<docx_rs::ParagraphChild> {
    use docx_rs::*;
    dd::spans(text)
        .into_iter()
        .map(|sp| {
            let mut r = Run::new().add_text(sp.text.clone());
            if let Some(s) = size {
                r = r.size(s);
            }
            if sp.bold {
                r = r.bold();
            }
            if sp.italic {
                r = r.italic();
            }
            if sp.code {
                r = r
                    .fonts(RunFonts::new().ascii("Consolas").hi_ansi("Consolas").cs("Consolas"))
                    .shading(Shading::new().shd_type(ShdType::Clear).color("auto").fill(theme.surface.clone()));
            }
            match (&sp.link, color) {
                (Some(url), _) => ParagraphChild::Hyperlink(
                    Hyperlink::new(url.clone(), HyperlinkType::External).add_run(r.color(theme.accent.clone()).underline("single")),
                ),
                (None, Some(c)) => ParagraphChild::Run(Box::new(r.color(c))),
                (None, None) => ParagraphChild::Run(Box::new(r)),
            }
        })
        .collect()
}

fn docx_para(text: &str, size: Option<usize>, color: Option<&str>, theme: &Theme) -> docx_rs::Paragraph {
    let mut p = docx_rs::Paragraph::new();
    for child in docx_runs(text, size, color, theme) {
        p = match child {
            docx_rs::ParagraphChild::Run(r) => p.add_run(*r),
            docx_rs::ParagraphChild::Hyperlink(h) => p.add_hyperlink(h),
            _ => p,
        };
    }
    p
}

/// A one-cell table: the shape Word gives callouts, quotes and code.
fn docx_box(paras: Vec<docx_rs::Paragraph>, fill: Option<&str>, left: Option<(&str, usize)>) -> docx_rs::Table {
    use docx_rs::*;
    let mut cell = TableCell::new().width(5000, WidthType::Pct).clear_all_border();
    for p in paras {
        cell = cell.add_paragraph(p);
    }
    if let Some(f) = fill {
        cell = cell.shading(Shading::new().shd_type(ShdType::Clear).color("auto").fill(f));
    }
    if let Some((color, size)) = left {
        cell = cell.set_border(TableCellBorder::new(TableCellBorderPosition::Left).color(color).size(size).border_type(BorderType::Single));
    }
    Table::without_borders(vec![TableRow::new(vec![cell])]).width(5000, WidthType::Pct).margins(TableCellMargins::new().margin(110, 160, 110, 200))
}

/// A small, tracked-out uppercase label: table headers, eyebrows, the
/// labels of numbers.
fn docx_label(text: &str, color: &str) -> docx_rs::Paragraph {
    use docx_rs::*;
    Paragraph::new().add_run(Run::new().add_text(dd::plain(text).to_uppercase()).size(14).bold().color(color).character_spacing(24))
}

fn docx_spacer() -> docx_rs::Paragraph {
    docx_rs::Paragraph::new().line_spacing(docx_rs::LineSpacing::new().before(0).after(80))
}

fn docx_picture(bytes: &[u8], w_px: u32, h_px: u32, width_mm: f32) -> docx_rs::Paragraph {
    use docx_rs::*;
    let w_emu = (width_mm * EMU_MM as f32) as u32;
    let h_emu = (w_emu as f64 * h_px as f64 / w_px.max(1) as f64) as u32;
    let pic = Pic::new_with_dimensions(bytes.to_vec(), w_px, h_px).size(w_emu, h_emu);
    Paragraph::new().align(AlignmentType::Center).add_run(Run::new().add_image(pic))
}

/// webp isn't understood by Word or PowerPoint: those become PNG.
fn office_image(img: &Image) -> Result<(Vec<u8>, &'static str), String> {
    match img.mime {
        "image/png" => Ok((img.bytes.clone(), "png")),
        "image/jpeg" => Ok((img.bytes.clone(), "jpeg")),
        "image/gif" => Ok((img.bytes.clone(), "gif")),
        _ => {
            let decoded = image::load_from_memory(&img.bytes).map_err(|e| format!("couldn't convert {}: {e}", img.path.display()))?;
            let mut out = Vec::new();
            decoded.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png).map_err(|e| e.to_string())?;
            Ok((out, "png"))
        }
    }
}

fn build_docx(path: &Path, meta: &DocMeta, blocks: &[Block], theme: &Theme, notes: &mut Vec<String>) -> Result<(), String> {
    use docx_rs::*;

    let (page_w, page_h) = dd::page_mm(meta);
    let text_w_mm = page_w - 38.0;
    let fonts = |f: &str| RunFonts::new().ascii(f).hi_ansi(f).cs(f).east_asia(f);
    let heading = |id: &str, name: &str, size: usize, color: &str, level: usize, before: u32, after: u32| {
        Style::new(id, StyleType::Paragraph)
            .name(name)
            .based_on("Normal")
            .next("Normal")
            .q_format(true)
            .size(size)
            .bold()
            .color(color)
            .fonts(fonts(&theme.heading_font))
            .outline_lvl(level)
            .line_spacing(LineSpacing::new().before(before).after(after))
    };

    let mut docx = Docx::new()
        .page_size((page_w * TWIP_MM) as u32, (page_h * TWIP_MM) as u32)
        .page_margin(PageMargin { top: 1134, left: 1077, bottom: 1247, right: 1077, header: 567, footer: 567, gutter: 0 })
        .default_fonts(fonts(&theme.body_font))
        .default_size(21)
        .add_style(Style::new("Normal", StyleType::Paragraph).name("Normal").color(theme.ink.clone()).line_spacing(LineSpacing::new().line(276).after(120)))
        .add_style(heading("Title", "Title", 52, &theme.ink, 0, 0, 60))
        .add_style(Style::new("Subtitle", StyleType::Paragraph).name("Subtitle").based_on("Normal").size(28).color(theme.muted.clone()))
        .add_style(heading("Heading1", "heading 1", 29, &theme.ink, 0, 400, 120))
        .add_style(heading("Heading2", "heading 2", 26, &theme.ink, 1, 240, 80))
        .add_style(heading("Heading3", "heading 3", 15, &theme.muted, 2, 240, 80))
        .add_style(Style::new("Caption", StyleType::Paragraph).name("caption").based_on("Normal").size(15).color(theme.muted.clone()));

    // Lists: one bullet definition; each numbered list gets its own instance
    // so it starts again at 1.
    let level = |l: usize, fmt: &str, text: &str| {
        Level::new(l, Start::new(1), NumberFormat::new(fmt), LevelText::new(text), LevelJc::new("left"))
            .indent(Some(360 * (l as i32 + 1)), Some(SpecialIndentType::Hanging(300)), None, None)
    };
    docx = docx
        .add_abstract_numbering(AbstractNumbering::new(1).add_level(level(0, "bullet", "•")).add_level(level(1, "bullet", "◦")).add_level(level(2, "bullet", "▪")))
        .add_abstract_numbering(AbstractNumbering::new(2).add_level(level(0, "decimal", "%1.")).add_level(level(1, "lowerLetter", "%2.")).add_level(level(2, "lowerRoman", "%3.")))
        .add_numbering(Numbering::new(1, 1));
    let mut next_num = 2usize;

    // Title block or cover.
    let byline: String = [meta.author.as_str(), meta.date.as_str()].iter().filter(|s| !s.is_empty()).copied().collect::<Vec<_>>().join("  ·  ");
    if !meta.title.is_empty() {
        if meta.cover {
            let top = if meta.eyebrow.is_empty() { meta.author.clone() } else { meta.eyebrow.clone() };
            docx = docx.add_paragraph(docx_label(&top, &theme.muted).line_spacing(LineSpacing::new().after(4600)));
        } else if !meta.eyebrow.is_empty() {
            docx = docx.add_paragraph(docx_label(&meta.eyebrow, &theme.accent).line_spacing(LineSpacing::new().after(160)));
        }
        docx = docx.add_paragraph(docx_para(&meta.title, None, None, theme).style("Title"));
        if !meta.subtitle.is_empty() {
            docx = docx.add_paragraph(docx_para(&meta.subtitle, None, None, theme).style("Subtitle"));
        }
        if !byline.is_empty() {
            docx = docx.add_paragraph(Paragraph::new().add_run(Run::new().add_text(byline.clone()).size(18).color(theme.muted.clone())));
        }
        if meta.cover {
            docx = docx.add_paragraph(Paragraph::new().add_run(Run::new().add_break(BreakType::Page)));
        } else {
            docx = docx.add_table(
                Table::without_borders(vec![TableRow::new(vec![TableCell::new()
                    .add_paragraph(Paragraph::new())
                    .clear_all_border()
                    .set_border(TableCellBorder::new(TableCellBorderPosition::Top).color(theme.line.clone()).size(4).border_type(BorderType::Single))])])
                .width(5000, WidthType::Pct),
            );
        }
    }

    for block in blocks {
        match block {
            Block::Heading { text, level } => {
                docx = docx.add_paragraph(docx_para(text, None, None, theme).style(&format!("Heading{level}")).keep_next(true));
            }
            Block::Paragraph { text } => docx = docx.add_paragraph(docx_para(text, None, None, theme)),
            Block::List { items, ordered } => {
                let num = if *ordered {
                    let id = next_num;
                    next_num += 1;
                    docx = docx.add_numbering(Numbering::new(id, 2).add_override(LevelOverride::new(0).start(1)));
                    id
                } else {
                    1
                };
                for (text, l) in items {
                    docx = docx.add_paragraph(
                        docx_para(text, None, None, theme)
                            .numbering(NumberingId::new(num), IndentLevel::new(*l as usize))
                            .line_spacing(LineSpacing::new().line(264).after(40)),
                    );
                }
                docx = docx.add_paragraph(docx_spacer());
            }
            Block::Table { rows, header, caption, align } => {
                if rows.is_empty() {
                    continue;
                }
                let cols = rows.iter().map(|r| r.len()).max().unwrap_or(1).max(1);
                let col_w = (text_w_mm * TWIP_MM) as usize / cols;
                let aligns = dd::column_align(rows, *header, align);
                let table_rows: Vec<TableRow> = rows
                    .iter()
                    .enumerate()
                    .map(|(ri, row)| {
                        let head = *header && ri == 0;
                        TableRow::new(
                            (0..cols)
                                .map(|ci| {
                                    let cell_text = row.get(ci).map(String::as_str).unwrap_or("");
                                    let mut p = if head { docx_label(cell_text, &theme.muted) } else { docx_para(cell_text, Some(18), None, theme) }.line_spacing(LineSpacing::new().after(0));
                                    p = match aligns.get(ci).copied() {
                                        Some("right") => p.align(AlignmentType::Right),
                                        Some("center") => p.align(AlignmentType::Center),
                                        _ => p,
                                    };
                                    let (size, color) = if head { (10, theme.ink.clone()) } else { (4, theme.line.clone()) };
                                    TableCell::new()
                                        .add_paragraph(p)
                                        .width(col_w, WidthType::Dxa)
                                        .vertical_align(if head { VAlignType::Bottom } else { VAlignType::Top })
                                        .clear_all_border()
                                        .set_border(TableCellBorder::new(TableCellBorderPosition::Bottom).color(color).size(size).border_type(BorderType::Single))
                                })
                                .collect(),
                        )
                    })
                    .collect();
                docx = docx.add_table(Table::without_borders(table_rows).width(5000, WidthType::Pct).set_grid(vec![col_w; cols]).margins(TableCellMargins::new().margin(90, 120, 90, 120)));
                if !caption.is_empty() {
                    docx = docx.add_paragraph(docx_para(caption, None, None, theme).style("Caption"));
                } else {
                    docx = docx.add_paragraph(docx_spacer());
                }
            }
            Block::Image { image, caption, width_pct } => {
                let (bytes, _) = office_image(image)?;
                docx = docx.add_paragraph(docx_picture(&bytes, image.width, image.height, text_w_mm * *width_pct as f32 / 100.0));
                if !caption.is_empty() {
                    docx = docx.add_paragraph(docx_para(caption, None, None, theme).style("Caption"));
                }
            }
            Block::Quote { text, by } => {
                let serif = RunFonts::new().ascii("Georgia").hi_ansi("Georgia").cs("Georgia");
                let q = |t: &str, c: &str| Run::new().add_text(t).size(30).italic().color(c).fonts(serif.clone());
                let p = Paragraph::new()
                    .line_spacing(LineSpacing::new().before(240).after(100).line(300))
                    .add_run(q("“", &theme.accent))
                    .add_run(q(&dd::plain(text), &theme.ink))
                    .add_run(q("”", &theme.accent));
                docx = docx.add_paragraph(p);
                if !by.is_empty() {
                    docx = docx.add_paragraph(docx_label(by, &theme.muted).line_spacing(LineSpacing::new().after(280)));
                }
            }
            Block::Callout { tone, title, text } => {
                let mut paras = Vec::new();
                if !title.is_empty() {
                    paras.push(docx_label(title, dd::tone_color(tone)).line_spacing(LineSpacing::new().after(60)));
                }
                paras.push(docx_para(text, None, None, theme).line_spacing(LineSpacing::new().after(0)));
                docx = docx.add_table(docx_box(paras, Some(&theme.surface), None)).add_paragraph(docx_spacer());
            }
            Block::Code { text, .. } => {
                let paras = text
                    .lines()
                    .map(|l| {
                        Paragraph::new()
                            .line_spacing(LineSpacing::new().line(240).after(0))
                            .add_run(Run::new().add_text(l).size(18).fonts(RunFonts::new().ascii("Consolas").hi_ansi("Consolas").cs("Consolas")))
                    })
                    .collect();
                docx = docx.add_table(docx_box(paras, Some(&theme.surface), None)).add_paragraph(docx_spacer());
            }
            Block::Metrics { items } => {
                if items.is_empty() {
                    continue;
                }
                let cells: Vec<TableCell> = items
                    .iter()
                    .enumerate()
                    .map(|(k, m)| {
                        let mut c = TableCell::new()
                            .clear_all_border()
                            .set_border(TableCellBorder::new(TableCellBorderPosition::Top).color(theme.ink.clone()).size(10).border_type(BorderType::Single))
                            .add_paragraph(docx_label(&m.label, &theme.muted).line_spacing(LineSpacing::new().before(60).after(60)))
                            .add_paragraph(docx_para(&m.value, Some(42), Some(&theme.ink), theme).line_spacing(LineSpacing::new().after(20)));
                        if k > 0 {
                            c = c.set_border(TableCellBorder::new(TableCellBorderPosition::Left).color(theme.line.clone()).size(4).border_type(BorderType::Single));
                        }
                        if !m.note.is_empty() {
                            c = c.add_paragraph(docx_para(&m.note, Some(16), Some(&theme.muted), theme));
                        }
                        c
                    })
                    .collect();
                docx = docx
                    .add_table(Table::without_borders(vec![TableRow::new(cells)]).width(5000, WidthType::Pct).margins(TableCellMargins::new().margin(60, 200, 60, 200)))
                    .add_paragraph(docx_spacer());
            }
            Block::Chart { chart } => {
                if !chart.title.is_empty() {
                    docx = docx.add_paragraph(docx_para(&chart.title, Some(19), None, theme).bold().keep_next(true));
                }
                match dd::chart_png(chart, theme, 640, 260) {
                    Ok(png) => docx = docx.add_paragraph(docx_picture(&png, 1280, 520, text_w_mm)),
                    Err(e) => {
                        notes.push(format!("chart \"{}\" is a table because {e}", chart.title));
                        docx = docx.add_table(chart_as_table(chart));
                    }
                }
            }
            Block::Divider => {
                docx = docx.add_table(
                    Table::without_borders(vec![TableRow::new(vec![TableCell::new()
                        .add_paragraph(Paragraph::new())
                        .clear_all_border()
                        .set_border(TableCellBorder::new(TableCellBorderPosition::Top).color(theme.line.clone()).size(6).border_type(BorderType::Single))])])
                    .width(5000, WidthType::Pct),
                );
            }
            Block::PageBreak => docx = docx.add_paragraph(Paragraph::new().add_run(Run::new().add_break(BreakType::Page))),
        }
    }

    let footer_text = if meta.footer.is_empty() { meta.title.clone() } else { meta.footer.clone() };
    docx = docx.footer(
        Footer::new().add_paragraph(
            Paragraph::new()
                .align(AlignmentType::Right)
                .add_run(Run::new().add_text(if footer_text.is_empty() { String::new() } else { format!("{}     ", footer_text.to_uppercase()) }).size(13).color(theme.faint.clone()).character_spacing(20))
                .add_page_num(PageNum::new()),
        ),
    );

    let file = std::fs::File::create(path).map_err(|e| format!("couldn't create file: {e}"))?;
    docx.build().pack(file).map_err(|e| format!("docx write failed: {e}"))?;
    Ok(())
}

fn chart_as_table(chart: &Chart) -> docx_rs::Table {
    use docx_rs::*;
    let mut rows = vec![TableRow::new(
        std::iter::once(String::new())
            .chain(chart.series.iter().map(|s| s.name.clone()))
            .map(|t| TableCell::new().add_paragraph(Paragraph::new().add_run(Run::new().add_text(t).bold())))
            .collect(),
    )];
    for (i, l) in chart.labels.iter().enumerate() {
        rows.push(TableRow::new(
            std::iter::once(l.clone())
                .chain(chart.series.iter().map(|s| s.values.get(i).map(|v| v.to_string()).unwrap_or_default()))
                .map(|t| TableCell::new().add_paragraph(Paragraph::new().add_run(Run::new().add_text(t))))
                .collect(),
        ));
    }
    Table::new(rows).width(5000, WidthType::Pct)
}

// ── XLSX ──────────────────────────────────────────────────────────────────

fn col_index(letters: &str) -> Option<u16> {
    let mut n: u32 = 0;
    for c in letters.trim().chars() {
        if !c.is_ascii_alphabetic() {
            return None;
        }
        n = n * 26 + (c.to_ascii_uppercase() as u32 - 'A' as u32 + 1);
    }
    (n > 0 && n <= 16_384).then(|| (n - 1) as u16)
}

/// "F2" → (row 1, col 5).
fn cell_ref(s: &str) -> Option<(u32, u16)> {
    let split = s.find(|c: char| c.is_ascii_digit())?;
    let col = col_index(&s[..split])?;
    let row: u32 = s[split..].parse().ok()?;
    (row > 0).then(|| (row - 1, col))
}

fn col_letter(mut c: u16) -> String {
    let mut s = String::new();
    loop {
        s.insert(0, (b'A' + (c % 26) as u8) as char);
        if c < 26 {
            break;
        }
        c = c / 26 - 1;
    }
    s
}

fn rgb(hex: &str) -> rust_xlsxwriter::Color {
    rust_xlsxwriter::Color::RGB(u32::from_str_radix(hex, 16).unwrap_or(0))
}

fn num_format(kind: &str, currency: &str) -> Option<String> {
    Some(match kind {
        "currency" | "money" => format!("{c}#,##0.00;[Red]-{c}#,##0.00", c = if currency.is_empty() { "$" } else { currency }),
        "percent" | "%" => "0.0%".into(),
        "number" | "decimal" => "#,##0.00".into(),
        "integer" | "int" => "#,##0".into(),
        "date" => "yyyy-mm-dd".into(),
        "datetime" => "yyyy-mm-dd hh:mm".into(),
        _ => return None,
    })
}

fn build_xlsx(path: &Path, sheets: &Value, theme: &Theme) -> Result<(), String> {
    use rust_xlsxwriter::*;

    let sheets = sheets.as_array().ok_or("`sheets` must be an array for xlsx")?;
    if sheets.is_empty() {
        return Err("`sheets` was empty — an xlsx needs at least one sheet".to_string());
    }
    let mut workbook = Workbook::new();
    let font = theme.body_font.trim_end_matches(" Semibold").to_string();
    let base = Format::new().set_font_name(&font).set_font_size(10.5);
    let border = rgb(&theme.line);

    for (si, sheet) in sheets.iter().enumerate() {
        let name: String = sheet
            .get("name")
            .and_then(|n| n.as_str())
            .map(|n| n.chars().filter(|c| !"[]:*?/\\".contains(*c)).take(31).collect())
            .filter(|n: &String| !n.trim().is_empty())
            .unwrap_or_else(|| format!("Sheet{}", si + 1));
        let worksheet = workbook.add_worksheet();
        worksheet.set_name(&name).map_err(|e| format!("sheet {si} name rejected: {e}"))?;

        let rows = sheet.get("rows").and_then(|r| r.as_array()).ok_or(format!("sheet {si} has no `rows` array"))?;
        let header = sheet.get("header").and_then(|h| h.as_bool()).unwrap_or(true);
        let columns: Vec<&Value> = sheet.get("columns").and_then(|c| c.as_array()).map(|a| a.iter().collect()).unwrap_or_default();
        let currency = sheet.get("currency").and_then(|c| c.as_str()).unwrap_or("");
        let ncols = rows.iter().filter_map(|r| r.as_array()).map(|r| r.len()).max().unwrap_or(0).max(columns.len());
        let col_kind = |c: usize| columns.get(c).and_then(|v| v.get("format")).and_then(|f| f.as_str()).unwrap_or("");

        // A quiet header: bold ink on the page's surface tone over an ink
        // rule; rows divided by hairlines, no banding.
        let header_fmt = base.clone().set_bold().set_font_color(rgb(&theme.ink)).set_background_color(rgb(&theme.surface)).set_border_bottom(FormatBorder::Thin).set_border_bottom_color(rgb(&theme.ink)).set_align(FormatAlign::VerticalCenter);
        let cell_fmt = |c: usize, zebra: bool| {
            let mut f = base.clone().set_border_bottom(FormatBorder::Thin).set_border_bottom_color(border);
            let _ = zebra;
            if let Some(nf) = num_format(col_kind(c), currency) {
                f = f.set_num_format(nf);
            }
            f
        };

        let first_data = if header { 1 } else { 0 };
        for (r, row) in rows.iter().enumerate() {
            let Some(cells) = row.as_array() else { continue };
            let is_header = header && r == 0;
            let zebra = !is_header && (r - first_data) % 2 == 1;
            for (c, cell) in cells.iter().enumerate() {
                let (ri, ci) = (r as u32, c as u16);
                let mut fmt = if is_header { header_fmt.clone() } else { cell_fmt(c, zebra) };
                // `{value|formula, bold?, color?, fill?, format?}` for one-off styling.
                let (value, formula) = match cell {
                    Value::Object(o) => {
                        if o.get("bold").and_then(|b| b.as_bool()) == Some(true) {
                            fmt = fmt.set_bold();
                        }
                        if let Some(c) = o.get("color").and_then(|c| c.as_str()).and_then(dd::hex) {
                            fmt = fmt.set_font_color(rgb(&c));
                        }
                        if let Some(c) = o.get("fill").and_then(|c| c.as_str()).and_then(dd::hex) {
                            fmt = fmt.set_background_color(rgb(&c));
                        }
                        if let Some(nf) = o.get("format").and_then(|f| f.as_str()).and_then(|f| num_format(f, currency)) {
                            fmt = fmt.set_num_format(nf);
                        }
                        (o.get("value").cloned().unwrap_or(Value::Null), o.get("formula").and_then(|f| f.as_str()).map(String::from))
                    }
                    Value::String(s) if s.starts_with('=') && !is_header => (Value::Null, Some(s.clone())),
                    other => (other.clone(), None),
                };
                let res = if let Some(f) = formula {
                    worksheet.write_formula_with_format(ri, ci, Formula::new(f), &fmt).map(|_| ())
                } else {
                    match &value {
                        Value::Number(n) => worksheet.write_number_with_format(ri, ci, n.as_f64().unwrap_or(0.0), &fmt).map(|_| ()),
                        Value::Bool(b) => worksheet.write_boolean_with_format(ri, ci, *b, &fmt).map(|_| ()),
                        Value::Null => worksheet.write_blank(ri, ci, &fmt).map(|_| ()),
                        Value::String(s) if matches!(col_kind(c), "date" | "datetime") && !is_header => match ExcelDateTime::parse_from_str(s) {
                            Ok(d) => worksheet.write_datetime_with_format(ri, ci, &d, &fmt).map(|_| ()),
                            Err(_) => worksheet.write_string_with_format(ri, ci, s, &fmt).map(|_| ()),
                        },
                        other => worksheet.write_string_with_format(ri, ci, dd::cell_text(other), &fmt).map(|_| ()),
                    }
                };
                res.map_err(|e| format!("sheet '{name}' cell {}{}: {e}", col_letter(ci), ri + 1))?;
            }
        }

        let nrows = rows.len() as u32;
        let last_data = nrows.saturating_sub(1);
        // A totals row: `total_row: true` sums every numeric column, or
        // `{"label": "Total", "columns": {"C": "sum", "D": "average"}}`.
        let mut used_rows = nrows;
        if let Some(t) = sheet.get("total_row").filter(|t| !t.is_null() && t.as_bool() != Some(false)) {
            if nrows > first_data as u32 {
                let r = nrows;
                let label = t.get("label").and_then(|l| l.as_str()).unwrap_or("Total");
                let total_fmt = base.clone().set_bold().set_border_top(FormatBorder::Medium).set_border_top_color(rgb(&theme.accent));
                worksheet.write_string_with_format(r, 0, label, &total_fmt).map_err(|e| e.to_string())?;
                for c in 1..ncols as u16 {
                    let func = match t.get("columns").and_then(|m| m.get(col_letter(c))).and_then(|f| f.as_str()) {
                        Some(f) => f.to_uppercase(),
                        None if t.get("columns").is_none() => {
                            let numeric = rows.iter().skip(first_data).filter_map(|r| r.as_array()).filter_map(|r| r.get(c as usize)).any(|v| v.is_number() || v.as_str().is_some_and(|s| s.starts_with('=')));
                            if numeric { "SUM".into() } else { continue }
                        }
                        None => continue,
                    };
                    let func = match func.as_str() {
                        "AVG" | "MEAN" => "AVERAGE".to_string(),
                        f => f.to_string(),
                    };
                    let mut f = total_fmt.clone();
                    if let Some(nf) = num_format(col_kind(c as usize), currency) {
                        f = f.set_num_format(nf);
                    }
                    let l = col_letter(c);
                    worksheet
                        .write_formula_with_format(r, c, Formula::new(format!("={func}({l}{}:{l}{})", first_data + 1, nrows)), &f)
                        .map_err(|e| e.to_string())?;
                }
                used_rows += 1;
            }
        }

        if header && nrows > 0 && ncols > 0 {
            worksheet.set_row_height(0, 22).map_err(|e| e.to_string())?;
            if sheet.get("freeze").and_then(|f| f.as_bool()).unwrap_or(true) {
                worksheet.set_freeze_panes(1, 0).map_err(|e| e.to_string())?;
            }
            if sheet.get("filter").and_then(|f| f.as_bool()).unwrap_or(true) && last_data > 0 {
                worksheet.autofilter(0, 0, last_data, (ncols - 1) as u16).map_err(|e| e.to_string())?;
            }
        }
        worksheet.autofit();
        // Autofit can't see what a formula will show, so columns of numbers
        // get room for totals (and never show ####).
        for c in 0..ncols {
            let numeric = rows.iter().skip(first_data).filter_map(|r| r.as_array()).filter_map(|r| r.get(c)).any(|v| v.is_number() || v.as_str().is_some_and(|s| s.starts_with('=')) || v.get("formula").is_some());
            if numeric {
                let header_len = rows.first().and_then(|r| r.get(c)).map(|v| dd::cell_text(v).chars().count()).unwrap_or(0) as f64;
                worksheet.set_column_width(c as u16, (header_len + 4.0).max(14.0)).map_err(|e| e.to_string())?;
            }
        }
        for (c, col) in columns.iter().enumerate() {
            if let Some(w) = col.get("width").and_then(|w| w.as_f64()) {
                worksheet.set_column_width(c as u16, w.clamp(2.0, 120.0)).map_err(|e| e.to_string())?;
            }
        }

        // Native charts, live over the sheet's own data.
        for (k, ch) in sheet.get("charts").and_then(|c| c.as_array()).into_iter().flatten().enumerate() {
            let kind = match ch.get("type").and_then(|t| t.as_str()).unwrap_or("column") {
                "bar" => ChartType::Bar,
                "line" => ChartType::Line,
                "pie" => ChartType::Pie,
                "doughnut" | "donut" => ChartType::Doughnut,
                "area" => ChartType::Area,
                "scatter" => ChartType::Scatter,
                _ => ChartType::Column,
            };
            let cats = ch.get("categories").and_then(|c| c.as_str()).and_then(col_index).ok_or(format!("sheet '{name}' chart {k}: `categories` must be a column letter, e.g. \"A\""))?;
            let values: Vec<u16> = ch.get("values").and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|v| v.as_str()).filter_map(col_index).collect()).unwrap_or_default();
            if values.is_empty() {
                return Err(format!("sheet '{name}' chart {k}: `values` must list column letters, e.g. [\"B\", \"C\"]"));
            }
            let (r0, r1) = (first_data as u32, last_data);
            let mut chart = Chart::new(kind);
            let pal = theme.palette();
            for (i, v) in values.iter().enumerate() {
                let series = chart.add_series();
                series.set_categories((name.as_str(), r0, cats, r1, cats)).set_values((name.as_str(), r0, *v, r1, *v));
                if header {
                    series.set_name((name.as_str(), 0, *v));
                }
                if !matches!(kind, ChartType::Pie | ChartType::Doughnut) {
                    let fill = rgb(&pal[i % pal.len()]);
                    series.set_format(ChartFormat::new().set_solid_fill(ChartSolidFill::new().set_color(fill)).set_line(ChartLine::new().set_color(fill).set_width(2.25)));
                }
            }
            if let Some(t) = ch.get("title").and_then(|t| t.as_str()) {
                chart.title().set_name(t);
            }
            chart.legend().set_position(ChartLegendPosition::Bottom);
            if values.len() == 1 && !matches!(kind, ChartType::Pie | ChartType::Doughnut) {
                chart.legend().set_hidden();
            }
            chart.set_width(ch.get("width").and_then(|w| w.as_u64()).unwrap_or(560) as u32);
            chart.set_height(ch.get("height").and_then(|h| h.as_u64()).unwrap_or(300) as u32);
            let (ar, ac) = ch
                .get("anchor")
                .and_then(|a| a.as_str())
                .and_then(cell_ref)
                .unwrap_or((1 + k as u32 * 17, ncols as u16 + 1));
            worksheet.insert_chart(ar, ac, &chart).map_err(|e| format!("sheet '{name}' chart {k}: {e}"))?;
        }
        let _ = used_rows;
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

const SLIDE_W: i64 = 12_192_000;
const SLIDE_H: i64 = 6_858_000;
const IN: f64 = 914_400.0;

fn emu(inches: f64) -> i64 {
    (inches * IN) as i64
}

/// One paragraph of a text box.
struct PPara {
    text: String,
    size: u32,
    color: String,
    bold: bool,
    italic: bool,
    font: String,
    bullet: Option<u8>,
    align: &'static str,
    space_before: u32,
    /// Letter-spacing in hundredths of a point (labels are tracked out).
    spacing: i32,
    caps: bool,
}

impl PPara {
    fn new(text: &str, size: u32, color: &str, font: &str) -> PPara {
        PPara { text: text.to_string(), size, color: color.to_string(), bold: false, italic: false, font: font.to_string(), bullet: None, align: "l", space_before: 0, spacing: 0, caps: false }
    }
    fn bold(mut self) -> Self {
        self.bold = true;
        self
    }
    fn italic(mut self) -> Self {
        self.italic = true;
        self
    }
    fn before(mut self, pts: u32) -> Self {
        self.space_before = pts;
        self
    }
    /// A small tracked-out uppercase label.
    fn label(mut self) -> Self {
        self.caps = true;
        self.bold = true;
        self.spacing = 150;
        self
    }
    fn tracking(mut self, spc: i32) -> Self {
        self.spacing = spc;
        self
    }
    /// `accent` colours links; `marker` colours bullets.
    fn xml(&self, accent: &str, marker: &str) -> String {
        let ppr = match self.bullet {
            Some(l) => format!(
                "<a:pPr marL=\"{}\" indent=\"-228600\" algn=\"{}\"><a:spcBef><a:spcPts val=\"{}\"/></a:spcBef><a:buClr><a:srgbClr val=\"{marker}\"/></a:buClr><a:buFont typeface=\"Arial\"/><a:buChar char=\"–\"/></a:pPr>",
                228600 + l as i64 * 320040,
                self.align,
                self.space_before * 100,
            ),
            None => format!("<a:pPr algn=\"{}\"><a:spcBef><a:spcPts val=\"{}\"/></a:spcBef><a:buNone/></a:pPr>", self.align, self.space_before * 100),
        };
        let runs: String = dd::spans(&self.text)
            .iter()
            .map(|sp| {
                let font = if sp.code { "Consolas".to_string() } else { self.font.clone() };
                format!(
                    "<a:r><a:rPr lang=\"en-US\" sz=\"{}\" b=\"{}\" i=\"{}\"{}{}{} dirty=\"0\"><a:solidFill><a:srgbClr val=\"{}\"/></a:solidFill><a:latin typeface=\"{}\"/><a:cs typeface=\"{}\"/></a:rPr><a:t>{}</a:t></a:r>",
                    self.size,
                    if self.bold || sp.bold { 1 } else { 0 },
                    if self.italic || sp.italic { 1 } else { 0 },
                    if sp.link.is_some() { " u=\"sng\"" } else { "" },
                    if self.caps { " cap=\"all\"" } else { "" },
                    if self.spacing != 0 { format!(" spc=\"{}\"", self.spacing) } else { String::new() },
                    if sp.link.is_some() { accent } else { &self.color },
                    xml_escape(&font),
                    xml_escape(&font),
                    xml_escape(&sp.text)
                )
            })
            .collect();
        format!("<a:p>{ppr}{runs}<a:endParaRPr lang=\"en-US\" sz=\"{}\" dirty=\"0\"/></a:p>", self.size)
    }
}

struct SlideXml {
    shapes: String,
    next_id: u32,
    /// (relationship id, media part name)
    media: Vec<(String, String)>,
    accent: String,
    marker: String,
}

impl SlideXml {
    fn id(&mut self) -> u32 {
        self.next_id += 1;
        self.next_id
    }
    fn rect(&mut self, x: i64, y: i64, w: i64, h: i64, fill: &str) {
        let id = self.id();
        self.shapes.push_str(&format!(
            "<p:sp><p:nvSpPr><p:cNvPr id=\"{id}\" name=\"Shape {id}\"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x=\"{x}\" y=\"{y}\"/><a:ext cx=\"{w}\" cy=\"{h}\"/></a:xfrm><a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom><a:solidFill><a:srgbClr val=\"{fill}\"/></a:solidFill><a:ln><a:noFill/></a:ln></p:spPr></p:sp>"
        ));
    }
    /// A hairline: a thin rectangle reads the same in every viewer.
    fn rule(&mut self, x: i64, y: i64, w: i64, weight_pt: f64, color: &str) {
        self.rect(x, y, w, (weight_pt * 12_700.0) as i64, color);
    }
    fn text(&mut self, x: i64, y: i64, w: i64, h: i64, paras: &[PPara], anchor: &str, name: &str) {
        let id = self.id();
        let body: String = paras.iter().map(|p| p.xml(&self.accent, &self.marker)).collect();
        let body = if body.is_empty() { "<a:p><a:endParaRPr lang=\"en-US\"/></a:p>".to_string() } else { body };
        self.shapes.push_str(&format!(
            "<p:sp><p:nvSpPr><p:cNvPr id=\"{id}\" name=\"{name} {id}\"/><p:cNvSpPr txBox=\"1\"/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x=\"{x}\" y=\"{y}\"/><a:ext cx=\"{w}\" cy=\"{h}\"/></a:xfrm><a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom><a:noFill/></p:spPr><p:txBody><a:bodyPr wrap=\"square\" lIns=\"0\" tIns=\"0\" rIns=\"0\" bIns=\"0\" anchor=\"{anchor}\"><a:normAutofit/></a:bodyPr><a:lstStyle/>{body}</p:txBody></p:sp>"
        ));
    }
    fn picture(&mut self, part: String, x: i64, y: i64, w: i64, h: i64) {
        let id = self.id();
        let rid = format!("rId{}", self.media.len() + 2);
        self.media.push((rid.clone(), part));
        self.shapes.push_str(&format!(
            "<p:pic><p:nvPicPr><p:cNvPr id=\"{id}\" name=\"Picture {id}\"/><p:cNvPicPr><a:picLocks noChangeAspect=\"1\"/></p:cNvPicPr><p:nvPr/></p:nvPicPr><p:blipFill><a:blip r:embed=\"{rid}\"/><a:stretch><a:fillRect/></a:stretch></p:blipFill><p:spPr><a:xfrm><a:off x=\"{x}\" y=\"{y}\"/><a:ext cx=\"{w}\" cy=\"{h}\"/></a:xfrm><a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></p:spPr></p:pic>"
        ));
    }
    /// The document table, in PowerPoint: small tracked header over an ink
    /// rule, hairlines between rows, numbers right-aligned under their headers.
    fn table(&mut self, rows: &[Vec<String>], x: i64, y: i64, w: i64, theme: &Theme) {
        let cols = rows.iter().map(|r| r.len()).max().unwrap_or(1).max(1);
        let col_w = w / cols as i64;
        let row_h = emu(0.46);
        let size = if rows.len() > 9 { 1200 } else { 1400 };
        let aligns = dd::column_align(rows, true, &[]);
        let id = self.id();
        let grid: String = (0..cols).map(|_| format!("<a:gridCol w=\"{col_w}\"/>")).collect();
        let body: String = rows
            .iter()
            .enumerate()
            .map(|(ri, row)| {
                let head = ri == 0;
                let cells: String = (0..cols)
                    .map(|ci| {
                        let t = row.get(ci).cloned().unwrap_or_default();
                        let mut p = if head { PPara::new(&t, 1000, &theme.muted, &theme.body_font).label() } else { PPara::new(&t, size, &theme.ink, &theme.body_font) };
                        p.align = match aligns.get(ci).copied() {
                            Some("right") => "r",
                            Some("center") => "ctr",
                            _ => "l",
                        };
                        let (bw, bc) = if head { (12700, theme.ink.as_str()) } else { (6350, theme.line.as_str()) };
                        let mar_l = if ci == 0 { 0 } else { 91440 };
                        let mar_r = if ci + 1 == cols { 0 } else { 91440 };
                        format!(
                            "<a:tc><a:txBody><a:bodyPr/><a:lstStyle/>{}</a:txBody><a:tcPr marL=\"{mar_l}\" marR=\"{mar_r}\" marT=\"45720\" marB=\"45720\" anchor=\"{}\"><a:lnL w=\"0\"><a:noFill/></a:lnL><a:lnR w=\"0\"><a:noFill/></a:lnR><a:lnT w=\"0\"><a:noFill/></a:lnT><a:lnB w=\"{bw}\"><a:solidFill><a:srgbClr val=\"{bc}\"/></a:solidFill></a:lnB><a:noFill/></a:tcPr></a:tc>",
                            p.xml(&theme.accent, &theme.faint),
                            if head { "b" } else { "ctr" }
                        )
                    })
                    .collect();
                format!("<a:tr h=\"{row_h}\">{cells}</a:tr>")
            })
            .collect();
        let h = row_h * rows.len() as i64;
        self.shapes.push_str(&format!(
            "<p:graphicFrame><p:nvGraphicFramePr><p:cNvPr id=\"{id}\" name=\"Table {id}\"/><p:cNvGraphicFramePr><a:graphicFrameLocks noGrp=\"1\"/></p:cNvGraphicFramePr><p:nvPr/></p:nvGraphicFramePr><p:xfrm><a:off x=\"{x}\" y=\"{y}\"/><a:ext cx=\"{w}\" cy=\"{h}\"/></p:xfrm><a:graphic><a:graphicData uri=\"http://schemas.openxmlformats.org/drawingml/2006/table\"><a:tbl><a:tblPr/><a:tblGrid>{grid}</a:tblGrid>{body}</a:tbl></a:graphicData></a:graphic></p:graphicFrame>"
        ));
    }
}

/// Fit `w`×`h` pixels inside a box, keeping the aspect ratio, centred.
fn fit(w_px: u32, h_px: u32, x: i64, y: i64, bw: i64, bh: i64) -> (i64, i64, i64, i64) {
    let ar = w_px.max(1) as f64 / h_px.max(1) as f64;
    let (mut w, mut h) = (bw as f64, bw as f64 / ar);
    if h > bh as f64 {
        h = bh as f64;
        w = h * ar;
    }
    (x + (bw - w as i64) / 2, y + (bh - h as i64) / 2, w as i64, h as i64)
}

struct Media {
    part: String,
    bytes: Vec<u8>,
}

/// One slide, typography-led: the same system as the PDF deck (`deck_html`).
fn slide_xml(i: usize, s: &Slide, deck: &str, section_no: usize, theme: &Theme, media: &mut Vec<Media>, notes: &mut Vec<String>) -> Result<SlideXml, String> {
    let (ink, muted, faint, line) = (theme.ink.clone(), theme.muted.clone(), theme.faint.clone(), theme.line.clone());
    let hero = matches!(s.layout.as_str(), "title" | "closing");
    let section = s.layout == "section";
    let slide_bg = if section { theme.surface.clone() } else { theme.paper.clone() };
    let mut x = SlideXml { shapes: String::new(), next_id: 1, media: Vec::new(), accent: theme.accent.clone(), marker: faint.clone() };
    let m = emu(0.8);
    let cw = SLIDE_W - 2 * m;
    let hf = theme.heading_font.clone();
    let bf = theme.body_font.clone();
    // Big numbers in the sans even under serif headings.
    let nf = if theme.serif_headings() { bf.clone() } else { hf.clone() };

    let mut add_media = |x: &mut SlideXml, bytes: Vec<u8>, ext: &str, bx: (i64, i64, i64, i64), px: (u32, u32)| {
        let part = format!("image{}.{ext}", media.len() + 1);
        media.push(Media { part: part.clone(), bytes });
        let (px_x, px_y, pw, ph) = fit(px.0, px.1, bx.0, bx.1, bx.2, bx.3);
        x.picture(part, px_x, px_y, pw, ph);
    };
    let title_box = |x: &mut SlideXml| {
        if !s.title.is_empty() {
            x.text(m, emu(0.62), cw * 85 / 100, emu(1.0), &[PPara::new(&s.title, 2800, &ink, &hf).bold().tracking(-40)], "t", "Title");
        }
    };
    let lede = |x: &mut SlideXml| -> i64 {
        if s.subtitle.is_empty() {
            return 0;
        }
        x.text(m, emu(1.5), cw * 72 / 100, emu(0.6), &[PPara::new(&s.subtitle, 1500, &muted, &bf)], "t", "Lede");
        emu(0.55)
    };
    // Points: hairlines between them rather than bullets.
    let points = |x: &mut SlideXml, items: &[(String, u8)], left: i64, top: i64, width: i64, size: u32| {
        let mut y = top;
        let tops: Vec<usize> = items.iter().enumerate().filter(|(_, (_, l))| *l == 0).map(|(k, _)| k).collect();
        for (n, &k) in tops.iter().enumerate() {
            let end = tops.get(n + 1).copied().unwrap_or(items.len());
            let subs: Vec<&(String, u8)> = items[k + 1..end].iter().collect();
            let h = emu(0.62) + subs.len() as i64 * emu(0.32);
            x.rule(left, y, width, 0.6, &line);
            let mut paras = vec![PPara::new(&items[k].0, size, &ink, &bf)];
            for (t, _) in &subs {
                paras.push(PPara::new(t, size - 550, &muted, &bf).before(3));
            }
            x.text(left, y + emu(0.14), width, h - emu(0.14), &paras, "t", "Point");
            y += h;
        }
        if !tops.is_empty() {
            x.rule(left, y, width, 0.6, &line);
        }
    };
    let body_top = emu(1.95);
    let body_h = SLIDE_H - body_top - emu(0.85);

    match s.layout.as_str() {
        "title" | "closing" => {
            x.rule(m, emu(3.55), emu(0.5), 1.5, &theme.accent);
            x.text(m, emu(3.75), cw * 85 / 100, emu(1.7), &[PPara::new(&s.title, 4600, &ink, &hf).bold().tracking(-60)], "t", "Title");
            if !s.subtitle.is_empty() {
                x.text(m, emu(5.45), cw * 70 / 100, emu(0.9), &[PPara::new(&s.subtitle, 1700, &muted, &bf)], "t", "Subtitle");
            }
        }
        "section" => {
            x.text(m, emu(2.2), cw, emu(1.0), &[PPara::new(&format!("{section_no:02}"), 5400, &theme.accent, &nf).tracking(-80)], "b", "Number");
            x.text(m, emu(3.3), cw * 80 / 100, emu(1.0), &[PPara::new(&s.title, 3800, &ink, &hf).bold().tracking(-50)], "t", "Title");
            if !s.subtitle.is_empty() {
                x.text(m, emu(4.35), cw * 65 / 100, emu(0.8), &[PPara::new(&s.subtitle, 1600, &muted, &bf)], "t", "Subtitle");
            }
        }
        "two_column" => {
            title_box(&mut x);
            let off = lede(&mut x);
            let gap = emu(0.7);
            let colw = (cw - gap) / 2;
            for (k, (heading, items)) in [&s.left, &s.right].into_iter().enumerate() {
                let cx = m + k as i64 * (colw + gap);
                if !heading.is_empty() {
                    x.text(cx, body_top + off, colw, emu(0.35), &[PPara::new(heading, 1000, &theme.accent, &bf).label()], "b", "Heading");
                }
                points(&mut x, items, cx, body_top + off + emu(0.45), colw, 1800);
            }
        }
        "image" => {
            title_box(&mut x);
            let side = !s.bullets.is_empty() || !s.caption.is_empty();
            let bx = if side { (m, body_top, cw * 57 / 100, body_h) } else { (m, body_top, cw, body_h) };
            if let Some(img) = &s.image {
                let (bytes, ext) = office_image(img)?;
                add_media(&mut x, bytes, ext, bx, (img.width, img.height));
            }
            if side {
                let tx = m + cw * 57 / 100 + emu(0.5);
                let tw = SLIDE_W - tx - m;
                points(&mut x, &s.bullets, tx, body_top + emu(0.4), tw, 1700);
                if !s.caption.is_empty() {
                    let ty = body_top + emu(0.6) + s.bullets.iter().filter(|(_, l)| *l == 0).count() as i64 * emu(0.62);
                    x.text(tx, ty, tw, emu(1.2), &[PPara::new(&s.caption, 1200, &muted, &bf)], "t", "Caption");
                }
            }
        }
        "quote" => {
            let mut paras = vec![PPara::new(&format!("“{}”", s.quote), 3200, &ink, if theme.serif_headings() { &hf } else { "Georgia" }).italic().tracking(-20)];
            if !s.by.is_empty() {
                paras.push(PPara::new(&s.by, 1000, &muted, &bf).label().before(24));
            }
            x.text(m, emu(1.6), cw * 85 / 100, emu(4.2), &paras, "ctr", "Quote");
        }
        "stats" => {
            title_box(&mut x);
            let off = lede(&mut x);
            let n = s.stats.len().clamp(1, 4) as i64;
            let w = cw / n;
            let top = emu(2.7) + off;
            x.rule(m, top, cw, 1.0, &ink);
            for (k, st) in s.stats.iter().take(4).enumerate() {
                let sx = m + k as i64 * w;
                if k > 0 {
                    x.rect(sx, top + emu(0.12), 6350, emu(2.0), &line);
                }
                let pad = if k == 0 { 0 } else { emu(0.3) };
                let mut paras = vec![PPara::new(&st.label, 1000, &muted, &bf).label(), PPara::new(&st.value, 5400, &ink, &nf).tracking(-80).before(10)];
                if !st.note.is_empty() {
                    paras.push(PPara::new(&st.note, 1200, &muted, &bf).before(6));
                }
                x.text(sx + pad, top + emu(0.25), w - pad - emu(0.2), emu(2.2), &paras, "t", "Stat");
            }
        }
        "table" => {
            title_box(&mut x);
            let off = lede(&mut x);
            if !s.table.is_empty() {
                x.table(&s.table, m, body_top + off, cw, theme);
            }
        }
        "chart" => {
            title_box(&mut x);
            let off = lede(&mut x);
            if let Some(c) = &s.chart {
                match dd::chart_png(c, theme, 1100, 440) {
                    Ok(png) => add_media(&mut x, png, "png", (m, body_top + off, cw, body_h - off), (2200, 880)),
                    Err(e) => {
                        notes.push(format!("slide {}: the chart is a table because {e}", i + 1));
                        let mut rows = vec![std::iter::once(String::new()).chain(c.series.iter().map(|s| s.name.clone())).collect::<Vec<_>>()];
                        for (k, l) in c.labels.iter().enumerate() {
                            rows.push(std::iter::once(l.clone()).chain(c.series.iter().map(|s| s.values.get(k).map(|v| v.to_string()).unwrap_or_default())).collect());
                        }
                        x.table(&rows, m, body_top + off, cw, theme);
                    }
                }
            }
        }
        _ => {
            title_box(&mut x);
            let off = lede(&mut x);
            points(&mut x, &s.bullets, m, body_top + off, cw * 78 / 100, 1900);
        }
    }
    if !hero {
        // A quiet footer: the deck's name and the slide number.
        let foot = |t: &str, align: &'static str| PPara { align, ..PPara::new(t, 800, &faint, &bf).label() };
        x.text(m, SLIDE_H - emu(0.5), cw / 2, emu(0.25), &[foot(deck, "l")], "b", "Footer");
        x.text(m + cw / 2, SLIDE_H - emu(0.5), cw / 2, emu(0.25), &[foot(&(i + 1).to_string(), "r")], "b", "Slide Number");
    }
    x.shapes = format!("<p:bg><p:bgPr><a:solidFill><a:srgbClr val=\"{slide_bg}\"/></a:solidFill><a:effectLst/></p:bgPr></p:bg>{}", x.shapes);
    Ok(x)
}

fn theme_xml(theme: &Theme) -> String {
    let p = theme.palette();
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" name="Vivianne">
<a:themeElements>
<a:clrScheme name="Vivianne"><a:dk1><a:srgbClr val="{ink}"/></a:dk1><a:lt1><a:srgbClr val="FFFFFF"/></a:lt1><a:dk2><a:srgbClr val="{ink}"/></a:dk2><a:lt2><a:srgbClr val="{surface}"/></a:lt2><a:accent1><a:srgbClr val="{a1}"/></a:accent1><a:accent2><a:srgbClr val="{a2}"/></a:accent2><a:accent3><a:srgbClr val="{a3}"/></a:accent3><a:accent4><a:srgbClr val="{a4}"/></a:accent4><a:accent5><a:srgbClr val="{a5}"/></a:accent5><a:accent6><a:srgbClr val="{a6}"/></a:accent6><a:hlink><a:srgbClr val="{a1}"/></a:hlink><a:folHlink><a:srgbClr val="{a2}"/></a:folHlink></a:clrScheme>
<a:fontScheme name="Vivianne"><a:majorFont><a:latin typeface="{hf}"/><a:ea typeface=""/><a:cs typeface=""/></a:majorFont><a:minorFont><a:latin typeface="{bf}"/><a:ea typeface=""/><a:cs typeface=""/></a:minorFont></a:fontScheme>
<a:fmtScheme name="Office">
<a:fillStyleLst><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:fillStyleLst>
<a:lnStyleLst><a:ln w="6350"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln><a:ln w="12700"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln><a:ln w="19050"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln></a:lnStyleLst>
<a:effectStyleLst><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/></a:effectStyle></a:effectStyleLst>
<a:bgFillStyleLst><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:bgFillStyleLst>
</a:fmtScheme>
</a:themeElements>
</a:theme>"#,
        ink = theme.ink,
        surface = theme.surface,
        a1 = p[0],
        a2 = p[1],
        a3 = p[2],
        a4 = p[3],
        a5 = p[4],
        a6 = p[5],
        hf = xml_escape(&theme.heading_font),
        bf = xml_escape(&theme.body_font),
    )
}

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
<p:sldLayout xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" type="blank" preserve="1">
<p:cSld name="Blank"><p:spTree>
<p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr>
<p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/><a:chOff x="0" y="0"/><a:chExt cx="0" cy="0"/></a:xfrm></p:grpSpPr>
</p:spTree></p:cSld>
<p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr>
</p:sldLayout>"#;

fn build_pptx(path: &Path, slides: &[Slide], theme: &Theme, notes: &mut Vec<String>) -> Result<(), String> {
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    let mut media: Vec<Media> = Vec::new();
    let mut built: Vec<SlideXml> = Vec::new();
    let deck = dd::deck_name(slides);
    let mut section_no = 0;
    for (i, s) in slides.iter().enumerate() {
        if s.layout == "section" {
            section_no += 1;
        }
        built.push(slide_xml(i, s, &deck, section_no, theme, &mut media, notes)?);
    }

    let file = std::fs::File::create(path).map_err(|e| format!("couldn't create file: {e}"))?;
    let mut zip = zip::ZipWriter::new(file);
    let opts = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let mut add = |name: &str, data: &[u8]| -> Result<(), String> {
        zip.start_file(name, opts).map_err(|e| format!("zip {name}: {e}"))?;
        zip.write_all(data).map_err(|e| format!("zip write {name}: {e}"))?;
        Ok(())
    };

    // Every part must be declared or PowerPoint calls the file corrupt.
    let slide_overrides: String = (1..=built.len())
        .map(|i| format!(r#"<Override PartName="/ppt/slides/slide{i}.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slide+xml"/>"#))
        .collect();
    add(
        "[Content_Types].xml",
        format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
<Default Extension="xml" ContentType="application/xml"/>
<Default Extension="png" ContentType="image/png"/>
<Default Extension="jpeg" ContentType="image/jpeg"/>
<Default Extension="gif" ContentType="image/gif"/>
<Override PartName="/ppt/presentation.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml"/>
<Override PartName="/ppt/slideMasters/slideMaster1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slideMaster+xml"/>
<Override PartName="/ppt/slideLayouts/slideLayout1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slideLayout+xml"/>
<Override PartName="/ppt/theme/theme1.xml" ContentType="application/vnd.openxmlformats-officedocument.theme+xml"/>
{slide_overrides}
</Types>"#
        )
        .as_bytes(),
    )?;
    add(
        "_rels/.rels",
        br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="ppt/presentation.xml"/>
</Relationships>"#,
    )?;
    // Slide ids start at 256: PowerPoint reserves lower values.
    let sld_id_list: String = (1..=built.len()).map(|i| format!(r#"<p:sldId id="{}" r:id="rId{}"/>"#, 255 + i, i + 1)).collect();
    add(
        "ppt/presentation.xml",
        format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:presentation xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main">
<p:sldMasterIdLst><p:sldMasterId id="2147483648" r:id="rId1"/></p:sldMasterIdLst>
<p:sldIdLst>{sld_id_list}</p:sldIdLst>
<p:sldSz cx="{SLIDE_W}" cy="{SLIDE_H}"/><p:notesSz cx="6858000" cy="9144000"/>
</p:presentation>"#
        )
        .as_bytes(),
    )?;
    let pres_rels: String = (1..=built.len())
        .map(|i| format!(r#"<Relationship Id="rId{}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide{i}.xml"/>"#, i + 1))
        .collect();
    add(
        "ppt/_rels/presentation.xml.rels",
        format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster" Target="slideMasters/slideMaster1.xml"/>
{pres_rels}
</Relationships>"#
        )
        .as_bytes(),
    )?;
    add("ppt/theme/theme1.xml", theme_xml(theme).as_bytes())?;
    add("ppt/slideMasters/slideMaster1.xml", SLIDE_MASTER_XML.as_bytes())?;
    add(
        "ppt/slideMasters/_rels/slideMaster1.xml.rels",
        br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout" Target="../slideLayouts/slideLayout1.xml"/>
<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme" Target="../theme/theme1.xml"/>
</Relationships>"#,
    )?;
    add("ppt/slideLayouts/slideLayout1.xml", SLIDE_LAYOUT_XML.as_bytes())?;
    add(
        "ppt/slideLayouts/_rels/slideLayout1.xml.rels",
        br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster" Target="../slideMasters/slideMaster1.xml"/>
</Relationships>"#,
    )?;
    for m in &media {
        add(&format!("ppt/media/{}", m.part), &m.bytes)?;
    }

    for (i, s) in built.iter().enumerate() {
        let xml = format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:sld xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main">
<p:cSld>{bg}<p:spTree>
<p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr>
<p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/><a:chOff x="0" y="0"/><a:chExt cx="0" cy="0"/></a:xfrm></p:grpSpPr>
{shapes}
</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr>
</p:sld>"#,
            // The background is the first thing in the slide's shapes string.
            bg = &s.shapes[..s.shapes.find("</p:bg>").map(|e| e + "</p:bg>".len()).unwrap_or(0)],
            shapes = &s.shapes[s.shapes.find("</p:bg>").map(|e| e + "</p:bg>".len()).unwrap_or(0)..],
        );
        add(&format!("ppt/slides/slide{}.xml", i + 1), xml.as_bytes())?;
        let media_rels: String = s
            .media
            .iter()
            .map(|(rid, part)| format!(r#"<Relationship Id="{rid}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="../media/{part}"/>"#))
            .collect();
        add(
            &format!("ppt/slides/_rels/slide{}.xml.rels", i + 1),
            format!(
                r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout" Target="../slideLayouts/slideLayout1.xml"/>
{media_rels}
</Relationships>"#
            )
            .as_bytes(),
        )?;
    }
    zip.finish().map_err(|e| format!("zip finish: {e}"))?;
    Ok(())
}

// ── Tool ──────────────────────────────────────────────────────────────────

/// Pixels for a preview of the first page at screen resolution.
fn page_px(meta: &DocMeta) -> (u32, u32) {
    let (w, h) = dd::page_mm(meta);
    ((w * 96.0 / 25.4).round() as u32, (h * 96.0 / 25.4).round() as u32)
}

struct Outcome {
    /// Notes for the model (fallbacks taken, things to know).
    notes: Vec<String>,
    /// HTML of the first page/slide, for the preview image.
    preview: Option<(String, u32, u32)>,
}

fn render(format: &str, args: &Value, cwd: &Path, out: &Path) -> Result<Outcome, String> {
    let theme = Theme::from_arg(args.get("theme"));
    let meta = DocMeta::from_args(args);
    let mut notes = Vec::new();
    let mut preview = None;
    match format {
        "pdf" => {
            let html = if let Some(slides) = args.get("slides") {
                let slides = dd::parse_slides(slides, cwd)?;
                let html = dd::deck_html(&slides, &theme);
                preview = Some((html.clone(), 1280, 720));
                html
            } else if let Some(h) = args.get("html").and_then(|h| h.as_str()).filter(|h| !h.trim().is_empty()) {
                dd::custom_html(h, &meta, &theme)
            } else {
                dd::document_html(&meta, &dd::parse_blocks(args.get("blocks"), cwd)?, &theme)
            };
            if preview.is_none() {
                let (w, h) = page_px(&meta);
                preview = Some((html.clone(), w, h));
            }
            if let Err(e) = dd::print_pdf(&html, out) {
                // No browser: the plain renderer, which can't do slides or HTML.
                if args.get("slides").is_some() || args.get("html").is_some() {
                    return Err(format!("couldn't render the PDF: {e}"));
                }
                let blocks = dd::parse_blocks(args.get("blocks"), cwd)?;
                std::fs::write(out, build_pdf_plain(&meta.title, &blocks)?).map_err(|e| format!("couldn't write pdf: {e}"))?;
                notes.push(format!("Plain PDF: {e}, so it has no theme, images or charts, and only Latin text. Installing Edge or Chrome fixes this."));
                preview = None;
            }
        }
        "docx" => {
            let blocks = dd::parse_blocks(args.get("blocks"), cwd)?;
            build_docx(out, &meta, &blocks, &theme, &mut notes)?;
            let (w, h) = page_px(&meta);
            preview = Some((dd::document_html(&meta, &blocks, &theme), w, h));
        }
        "png" => {
            let html = args.get("html").and_then(|h| h.as_str()).filter(|h| !h.trim().is_empty()).ok_or("`html` is required for png: the design on its canvas")?;
            let size = args.get("size");
            let dim = |k: &str, d: u64| size.and_then(|s| s.get(k)).and_then(|v| v.as_u64()).unwrap_or(d).clamp(16, 4000) as u32;
            let (w, h) = (dim("width", 1080), dim("height", 1080));
            let page = dd::canvas_html(html, w, h);
            let png = dd::screenshot(&page, w, h, 2.0, false).map_err(|e| format!("couldn't render the image: {e}"))?;
            std::fs::write(out, &png).map_err(|e| format!("couldn't write png: {e}"))?;
            preview = Some((page, w, h));
        }
        "xlsx" => {
            let sheets = args.get("sheets").ok_or("`sheets` is required for xlsx")?;
            build_xlsx(out, sheets, &theme)?;
        }
        "pptx" => {
            let slides = dd::parse_slides(args.get("slides").ok_or("`slides` is required for pptx")?, cwd)?;
            build_pptx(out, &slides, &theme, &mut notes)?;
            preview = Some((dd::deck_html(&slides[..1], &theme), 1280, 720));
        }
        other => return Err(format!("unknown format '{other}': use pdf, docx, xlsx, pptx or png")),
    }
    Ok(Outcome { notes, preview })
}

#[async_trait]
impl Tool for CreateDocumentTool {
    fn name(&self) -> &str {
        "create_document"
    }

    async fn execute(&self, args: &Value, _cwd: &str) -> ToolResult {
        let Some(format) = args.get("format").and_then(|f| f.as_str()) else {
            return err("'format' is required: pdf, docx, xlsx, or pptx");
        };
        let Some(rel_path) = args.get("path").and_then(|p| p.as_str()).filter(|p| !p.trim().is_empty()) else {
            return err("'path' is required: where to write the document");
        };
        let base = std::path::PathBuf::from(&self.cwd);
        let path = {
            let p = Path::new(rel_path);
            if p.is_absolute() { p.to_path_buf() } else { base.join(p) }
        };
        if let Some(parent) = path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                return err(format!("couldn't create {}: {e}", parent.display()));
            }
        }
        let want_preview = args.get("preview").and_then(|p| p.as_bool()).unwrap_or(true);
        let args = args.clone();
        let format = format.to_lowercase();
        let out = path.clone();

        // CPU-bound and blocking (zip, rendering, the browser process).
        let result = tokio::task::spawn_blocking(move || -> Result<(u64, Outcome, Option<Vec<u8>>), String> {
            let outcome = render(&format, &args, &base, &out)?;
            let bytes = std::fs::metadata(&out).map(|m| m.len()).map_err(|e| format!("couldn't stat written file: {e}"))?;
            let shot = match (&outcome.preview, want_preview) {
                (Some((html, w, h)), true) => dd::screenshot(html, *w, *h, 1.0, false).ok(),
                _ => None,
            };
            Ok((bytes, outcome, shot))
        })
        .await;

        let display = path.display().to_string();
        match result {
            Ok(Ok((bytes, outcome, shot))) => {
                let mut content = format!("Created {display} ({bytes} bytes).");
                for n in &outcome.notes {
                    content.push_str(&format!("\nNote: {n}"));
                }
                let mut details = serde_json::json!({
                    "kind": "document",
                    "path": display,
                    "format": path.extension().and_then(|e| e.to_str()).unwrap_or(""),
                    "bytes": bytes,
                });
                if let Some(png) = shot {
                    use base64::Engine;
                    content.push_str("\nAttached: a preview of the first page. Check it: fix anything that looks wrong (overflowing text, an empty or crowded slide, a broken table) before telling the user it's done.");
                    details["image_base64"] = Value::String(base64::engine::general_purpose::STANDARD.encode(png));
                    details["media_type"] = Value::String("image/png".into());
                }
                ToolResult { content, is_error: false, details: Some(details) }
            }
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

    #[test]
    fn cell_refs_and_columns() {
        assert_eq!(col_index("A"), Some(0));
        assert_eq!(col_index("AA"), Some(26));
        assert_eq!(cell_ref("F2"), Some((1, 5)));
        assert_eq!(col_letter(0), "A");
        assert_eq!(col_letter(27), "AB");
    }

    #[test]
    fn wrap_breaks_on_width_and_keeps_all_words() {
        let lines = wrap("one two three four five", 9);
        assert!(lines.iter().all(|l| l.len() <= 9), "over-long line: {lines:?}");
        assert_eq!(lines.join(" "), "one two three four five");
    }

    /// An unescaped `&` in a slide is the classic way to produce a .pptx
    /// PowerPoint refuses to open.
    #[test]
    fn slide_text_is_escaped() {
        let p = PPara::new("Q1 & <Q2>", 1800, "000000", "Segoe UI");
        let xml = p.xml("4F46E5", "A1A1AA");
        assert!(xml.contains("Q1 &amp; &lt;Q2&gt;") && !xml.contains("Q1 & <"));
    }

    /// Every format, from one description, into a temp folder: each file must
    /// be a valid package (every XML part parses) that reads back.
    #[test]
    fn writes_every_format() {
        let dir = std::env::temp_dir().join(format!("viv_doc_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let blocks = serde_json::json!([
            { "type": "heading", "text": "Overview" },
            { "type": "paragraph", "text": "Revenue grew **18%** with *strong* `Q3` results. See [the site](https://example.com) & more." },
            { "type": "bullets", "items": ["First & foremost", { "text": "Second", "items": ["Nested <point>"] }] },
            { "type": "numbered", "items": ["One", "Two"] },
            { "type": "table", "rows": [["Region", "Revenue"], ["EMEA", 1200], ["APAC", 950.5]] },
            { "type": "callout", "tone": "warning", "title": "Heads up", "text": "Costs rose." },
            { "type": "quote", "text": "Ship it.", "by": "Someone" },
            { "type": "code", "text": "fn main() {}" },
            { "type": "metrics", "items": [{ "value": "$4.2M", "label": "Revenue", "note": "+18%" }] },
            { "type": "divider" },
            { "type": "page_break" },
            { "type": "heading", "text": "Next", "level": 2 }
        ]);
        let base = serde_json::json!({ "title": "Q3 Report", "subtitle": "Board pack", "author": "Viv", "cover": true, "theme": "classic" });
        let mut notes = Vec::new();

        let meta = DocMeta::from_args(&base);
        let parsed = dd::parse_blocks(Some(&blocks), &dir).unwrap();
        build_docx(&dir.join("out.docx"), &meta, &parsed, &Theme::preset("classic"), &mut notes).unwrap();
        std::fs::write(dir.join("plain.pdf"), build_pdf_plain("Q3", &parsed).unwrap()).unwrap();
        build_xlsx(
            &dir.join("out.xlsx"),
            &serde_json::json!([{
                "name": "Sales", "columns": [{}, { "format": "currency" }, { "format": "percent" }],
                "rows": [["Region", "Revenue", "Share"], ["EMEA", 1200, 0.56], ["APAC", 950.5, "=B3/SUM(B2:B3)"]],
                "total_row": true,
                "charts": [{ "type": "column", "title": "Revenue", "categories": "A", "values": ["B"], "anchor": "E2" }]
            }]),
            &Theme::preset("modern"),
        )
        .unwrap();
        let slides = dd::parse_slides(
            &serde_json::json!([
                { "title": "Q3 & beyond", "subtitle": "Board" },
                { "title": "Agenda", "bullets": ["Results", { "text": "Plans", "items": ["Hiring"] }] },
                { "title": "Split", "left": { "heading": "Now", "bullets": ["a"] }, "right": { "heading": "Next", "bullets": ["b"] } },
                { "title": "Numbers", "stats": [{ "value": "18%", "label": "Growth" }] },
                { "title": "Table", "table": [["A", "B"], ["1", "2"]] },
                { "quote": "Ship it.", "by": "Someone" },
                { "title": "Section", "subtitle": "Part two", "layout": "section" }
            ]),
            &dir,
        )
        .unwrap();
        build_pptx(&dir.join("out.pptx"), &slides, &Theme::preset("midnight"), &mut notes).unwrap();

        for name in ["out.docx", "out.xlsx", "out.pptx"] {
            let f = std::fs::File::open(dir.join(name)).unwrap();
            let mut z = zip::ZipArchive::new(f).unwrap();
            for i in 0..z.len() {
                let mut part = z.by_index(i).unwrap();
                let pname = part.name().to_string();
                if pname.ends_with(".xml") || pname.ends_with(".rels") {
                    let mut s = String::new();
                    std::io::Read::read_to_string(&mut part, &mut s).unwrap();
                    let mut reader = quick_xml::Reader::from_str(&s);
                    loop {
                        match reader.read_event() {
                            Ok(quick_xml::events::Event::Eof) => break,
                            Ok(_) => {}
                            Err(e) => panic!("{name}:{pname} isn't well-formed XML: {e}"),
                        }
                    }
                }
            }
        }
        let pptx = std::fs::read(dir.join("out.pptx")).unwrap();
        assert!(!pptx.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod samples {
    use super::*;

    /// Writes one rich sample of every format, through the real pipeline
    /// (browser included), for looking at by eye.
    #[test]
    #[ignore = "run manually: cargo test --lib samples -- --ignored --nocapture"]
    fn write_samples() {
        let dir = std::path::PathBuf::from(std::env::var("VIV_SAMPLES").unwrap_or_else(|_| std::env::temp_dir().join("viv_samples").display().to_string()));
        std::fs::create_dir_all(&dir).unwrap();
        let icon = std::env::current_dir().unwrap().join("icons").join("128x128@2x.png").display().to_string();
        let chart = serde_json::json!({ "type": "chart", "kind": "bar", "title": "Revenue by quarter ($k)", "labels": ["Q1", "Q2", "Q3", "Q4"], "series": [{ "name": "2025", "values": [820, 940, 1105, 1230] }, { "name": "2026", "values": [960, 1120, 1310, 1480] }] });
        let blocks = serde_json::json!([
            { "type": "heading", "text": "Executive summary" },
            { "type": "paragraph", "text": "Revenue grew **18% year over year** to $4.2M, driven by *enterprise* expansion in EMEA. Gross margin held at 71% despite higher hosting costs; see [the dashboard](https://example.com) for detail." },
            { "type": "metrics", "items": [{ "value": "$4.2M", "label": "Revenue", "note": "+18% YoY" }, { "value": "71%", "label": "Gross margin", "note": "flat" }, { "value": "312", "label": "Customers", "note": "+41 net new" }] },
            { "type": "heading", "text": "Highlights", "level": 2 },
            { "type": "bullets", "items": ["Closed 3 enterprise deals over $250k", { "text": "Launched self-serve onboarding", "items": ["Time to value down from 9 days to 2", "Activation up 23%"] }, "Hired a VP of Sales"] },
            chart,
            { "type": "table", "caption": "Table 1: Revenue by region", "rows": [["Region", "Q3", "Q4", "Change"], ["EMEA", 1240, 1480, "+19%"], ["North America", 1810, 1950, "+8%"], ["APAC", 630, 770, "+22%"]] },
            { "type": "callout", "tone": "warning", "title": "Risk", "text": "Two of the top ten accounts renew in Q1; both are in active conversations." },
            { "type": "quote", "text": "The new onboarding made the product click for our whole team.", "by": "Head of Ops, a customer" },
            { "type": "heading", "text": "Next quarter", "level": 2 },
            { "type": "numbered", "items": ["Expand the EMEA team", "Ship SSO and audit logs", "Raise prices for new customers by 10%"] },
            { "type": "image", "path": icon, "caption": "Figure 1: The product icon", "width": 30 },
            { "type": "code", "text": "SELECT region, SUM(revenue) FROM sales GROUP BY region;" }
        ]);
        let doc = |format: &str, theme: &str| serde_json::json!({ "format": format, "title": "Q4 Business Review", "subtitle": "Prepared for the board", "author": "Vivianne", "date": "October 2026", "eyebrow": "Board report", "cover": true, "theme": theme, "blocks": blocks });
        let slides = serde_json::json!([
            { "title": "Q4 Business Review", "subtitle": "Board meeting · October 2026" },
            { "title": "Agenda", "bullets": ["Results", "Customers", { "text": "Plans", "items": ["Hiring", "Product"] }, "Risks"] },
            { "title": "By the numbers", "stats": [{ "value": "$4.2M", "label": "Revenue", "note": "+18% YoY" }, { "value": "71%", "label": "Gross margin" }, { "value": "312", "label": "Customers", "note": "+41" }] },
            { "title": "Revenue by quarter", "chart": { "kind": "line", "labels": ["Q1", "Q2", "Q3", "Q4"], "series": [{ "name": "2025", "values": [820, 940, 1105, 1230] }, { "name": "2026", "values": [960, 1120, 1310, 1480] }] } },
            { "title": "Where we stand", "left": { "heading": "Going well", "bullets": ["Enterprise pipeline", "Activation"] }, "right": { "heading": "Watch", "bullets": ["Hosting costs", "Two big renewals"] } },
            { "title": "By region", "table": [["Region", "Q3", "Q4"], ["EMEA", "1,240", "1,480"], ["North America", "1,810", "1,950"], ["APAC", "630", "770"]] },
            { "title": "The product", "image": icon, "bullets": ["Self-serve onboarding", "SSO next quarter"] },
            { "quote": "The new onboarding made the product click for our whole team.", "by": "Head of Ops, a customer" },
            { "layout": "section", "title": "Next quarter", "subtitle": "Plans and asks" },
            { "layout": "closing", "title": "Thank you", "subtitle": "Questions?" }
        ]);
        let jobs = vec![
            ("report.pdf", doc("pdf", "modern")),
            ("report.docx", doc("docx", "classic")),
            ("report-editorial.pdf", doc("pdf", "editorial")),
            ("deck.pptx", serde_json::json!({ "format": "pptx", "theme": "modern", "slides": slides })),
            ("deck-dark.pdf", serde_json::json!({ "format": "pdf", "theme": "midnight", "slides": slides })),
            ("sales.xlsx", serde_json::json!({ "format": "xlsx", "theme": "forest", "sheets": [{
                "name": "Sales", "columns": [{}, { "format": "currency" }, { "format": "currency" }, { "format": "percent" }],
                "rows": [["Region", "Q3", "Q4", "Growth"], ["EMEA", 1240, 1480, "=C2/B2-1"], ["North America", 1810, 1950, "=C3/B3-1"], ["APAC", 630, 770, "=C4/B4-1"]],
                "total_row": { "label": "Total", "columns": { "B": "sum", "C": "sum" } },
                "charts": [{ "type": "column", "title": "Revenue by region", "categories": "A", "values": ["B", "C"], "anchor": "F2" }]
            }] })),
        ];
        for (name, args) in jobs {
            let out = dir.join(name);
            let o = render(args["format"].as_str().unwrap(), &args, &dir, &out).unwrap();
            if let Some((html, w, h)) = &o.preview {
                std::fs::write(dir.join(format!("{name}.preview.png")), dd::screenshot(html, *w, *h, 1.0, false).unwrap()).unwrap();
            }
            println!("{} {:?}", out.display(), o.notes);
        }
    }
}

#[cfg(test)]
mod bisect {
    use super::*;

    #[test]
    #[ignore = "manual: one docx per block type, for opening in Word"]
    fn docx_per_block() {
        let dir = std::path::PathBuf::from(std::env::var("VIV_SAMPLES").unwrap());
        let icon = std::env::current_dir().unwrap().join("icons").join("128x128@2x.png").display().to_string();
        let cases = serde_json::json!({
            "plain": [{ "type": "paragraph", "text": "Hello" }],
            "link": [{ "type": "paragraph", "text": "See [site](https://example.com)" }],
            "code_span": [{ "type": "paragraph", "text": "Use `x`" }],
            "bullets": [{ "type": "bullets", "items": ["a", { "text": "b", "items": ["c"] }] }],
            "numbered": [{ "type": "numbered", "items": ["a", "b"] }],
            "table": [{ "type": "table", "rows": [["A", "B"], ["1", "2"]] }],
            "image": [{ "type": "image", "path": icon }],
            "callout": [{ "type": "callout", "text": "x" }],
            "quote": [{ "type": "quote", "text": "x", "by": "y" }],
            "code": [{ "type": "code", "text": "a\nb" }],
            "metrics": [{ "type": "metrics", "items": [{ "value": "1", "label": "x" }] }],
            "divider": [{ "type": "divider" }],
            "heading": [{ "type": "heading", "text": "H" }]
        });
        for (name, blocks) in cases.as_object().unwrap() {
            let parsed = dd::parse_blocks(Some(blocks), &dir).unwrap();
            let meta = DocMeta { title: "T".into(), ..Default::default() };
            build_docx(&dir.join(format!("b_{name}.docx")), &meta, &parsed, &Theme::preset("modern"), &mut Vec::new()).unwrap();
        }
        let meta = DocMeta { title: "T".into(), cover: true, ..Default::default() };
        build_docx(&dir.join("b_cover.docx"), &meta, &[], &Theme::preset("modern"), &mut Vec::new()).unwrap();
        build_docx(&dir.join("b_empty.docx"), &DocMeta::default(), &[], &Theme::preset("modern"), &mut Vec::new()).unwrap();
    }
}

#[cfg(test)]
mod template_samples {
    use super::*;

    #[test]
    #[ignore = "manual: renders the guide's templates"]
    fn render_templates() {
        let dir = std::path::PathBuf::from(std::env::var("VIV_SAMPLES").unwrap());
        let t = |n: &str| std::fs::read_to_string(format!("../../vivianne-guides/design/templates/{n}.html")).unwrap();
        for (name, args) in [
            ("cv.pdf", serde_json::json!({ "format": "pdf", "html": t("cv") })),
            ("invoice.pdf", serde_json::json!({ "format": "pdf", "html": t("invoice") })),
            ("letter.pdf", serde_json::json!({ "format": "pdf", "html": t("letter") })),
            ("poster.png", serde_json::json!({ "format": "png", "html": t("poster"), "size": { "width": 1123, "height": 1587 } })),
            ("social.png", serde_json::json!({ "format": "png", "html": t("social"), "size": { "width": 1080, "height": 1350 } })),
        ] {
            render(args["format"].as_str().unwrap(), &args, &dir, &dir.join(name)).unwrap();
        }
    }
}

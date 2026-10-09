//! The design layer shared by every document format: themes, the content
//! vocabulary (blocks with inline formatting), and an HTML/CSS renderer that
//! the system's own Chromium (Edge on every Windows machine, Chrome, Brave…)
//! prints to PDF.
//!
//! Why HTML for PDF: a browser engine does real typography (kerning, line
//! breaking, every script and emoji), tables, images and SVG charts, and its
//! print engine paginates with running page numbers. Placing glyphs by hand
//! (the `printpdf` path, kept as the fallback when no browser is installed)
//! can't come close. The same engine also screenshots a page, so the agent
//! can look at what it made, and renders charts to PNG for Word and
//! PowerPoint, which keep their native, editable formats.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::Value;

// ── Themes ───────────────────────────────────────────────────────────────
//
// A theme is a design direction, not a colour swap: a type pairing, warm or
// cool neutrals, and one accent that is used sparingly (links, the lead
// series of a chart, an eyebrow label). Text stays near-black; structure
// comes from type, space and hairline rules rather than coloured blocks.

#[derive(Debug, Clone)]
pub struct Theme {
    pub name: String,
    /// Hex colours, no `#`.
    pub accent: String,
    pub accent2: String,
    /// Body text.
    pub ink: String,
    /// Secondary text: captions, labels.
    pub muted: String,
    /// Tertiary: running heads, the quietest labels.
    pub faint: String,
    /// The page.
    pub paper: String,
    /// Quiet fills: code, callouts.
    pub surface: String,
    /// Hairlines.
    pub line: String,
    /// Office font names (Word, PowerPoint, Excel), which must be installed
    /// where the file is opened.
    pub heading_font: String,
    pub body_font: String,
    /// CSS font stacks for PDF, where the fonts are embedded.
    pub heading_css: String,
    pub body_css: String,
    pub dark: bool,
}

pub const SANS_CSS: &str = "'Inter', 'Segoe UI Variable Text', 'Segoe UI', system-ui, -apple-system, 'Helvetica Neue', Arial, sans-serif";
pub const SERIF_CSS: &str = "'Source Serif 4', 'Source Serif Pro', Georgia, 'Cambria', 'Times New Roman', serif";
pub const MONO_CSS: &str = "'Cascadia Mono', 'Cascadia Code', Consolas, 'SF Mono', Menlo, monospace";

pub const PRESETS: &[&str] = &["modern", "editorial", "classic", "minimal", "warm", "forest", "midnight"];

impl Theme {
    pub fn preset(name: &str) -> Theme {
        // Cool neutrals by default; warm ones for the serif directions.
        let cool = |accent: &str| Theme {
            name: name.to_string(),
            accent: accent.into(),
            accent2: "0E8A7E".into(),
            ink: "18181B".into(),
            muted: "6B6B74".into(),
            faint: "A1A1AA".into(),
            paper: "FFFFFF".into(),
            surface: "F5F5F6".into(),
            line: "E4E4E7".into(),
            heading_font: "Aptos".into(),
            body_font: "Aptos".into(),
            heading_css: SANS_CSS.into(),
            body_css: SANS_CSS.into(),
            dark: false,
        };
        let warm = |accent: &str| Theme {
            ink: "1C1917".into(),
            muted: "6F6A64".into(),
            faint: "A8A29E".into(),
            surface: "F6F4F1".into(),
            line: "E7E3DD".into(),
            ..cool(accent)
        };
        match name {
            "editorial" => Theme {
                heading_font: "Georgia".into(),
                body_font: "Georgia".into(),
                heading_css: SERIF_CSS.into(),
                body_css: SERIF_CSS.into(),
                accent2: "3F6E8C".into(),
                ..warm("A6402B")
            },
            "classic" => Theme {
                heading_font: "Georgia".into(),
                heading_css: SERIF_CSS.into(),
                ink: "1A1F2B".into(),
                muted: "667085".into(),
                faint: "98A2B3".into(),
                surface: "F4F6F8".into(),
                line: "E4E7EC".into(),
                accent2: "9C7A3C".into(),
                ..cool("1F3A5F")
            },
            "minimal" => Theme { accent2: "71717A".into(), ..cool("18181B") },
            "warm" => Theme { heading_font: "Georgia".into(), heading_css: SERIF_CSS.into(), accent2: "4D7C6B".into(), ..warm("B4532A") },
            "forest" => Theme {
                ink: "17201B".into(),
                muted: "66706A".into(),
                faint: "9CA59F".into(),
                surface: "F3F6F4".into(),
                line: "E0E6E2".into(),
                accent2: "B08D3C".into(),
                ..cool("2F6B4F")
            },
            "midnight" => Theme {
                ink: "F4F4F5".into(),
                muted: "A1A1AA".into(),
                faint: "71717A".into(),
                paper: "0E1116".into(),
                surface: "171B22".into(),
                line: "2A2F38".into(),
                accent2: "5EC7B7".into(),
                dark: true,
                ..cool("8EA2FF")
            },
            _ => Theme { name: "modern".into(), ..cool("3451D1") },
        }
    }

    /// `theme` is a preset name, or `{preset?, accent?, accent2?,
    /// heading_font?, body_font?, dark?}` (a brand colour, say).
    pub fn from_arg(v: Option<&Value>) -> Theme {
        match v {
            Some(Value::String(s)) => Theme::preset(&s.to_lowercase()),
            Some(Value::Object(o)) => {
                let base = o.get("preset").and_then(|p| p.as_str()).unwrap_or(if o.get("dark").and_then(|d| d.as_bool()) == Some(true) { "midnight" } else { "modern" });
                let mut t = Theme::preset(&base.to_lowercase());
                if let Some(c) = o.get("accent").and_then(|c| c.as_str()).and_then(hex) {
                    t.accent = c;
                }
                if let Some(c) = o.get("accent2").and_then(|c| c.as_str()).and_then(hex) {
                    t.accent2 = c;
                }
                if let Some(f) = o.get("heading_font").and_then(|f| f.as_str()).filter(|f| !f.trim().is_empty()) {
                    t.heading_font = f.to_string();
                    t.heading_css = format!("'{}', {}", f.replace(['\'', '"'], ""), t.heading_css);
                }
                if let Some(f) = o.get("body_font").and_then(|f| f.as_str()).filter(|f| !f.trim().is_empty()) {
                    t.body_font = f.to_string();
                    t.body_css = format!("'{}', {}", f.replace(['\'', '"'], ""), t.body_css);
                }
                t
            }
            _ => Theme::preset("modern"),
        }
    }

    /// Chart series colours: the accent leads; comparisons are neutral, so
    /// the eye goes where the story is.
    pub fn palette(&self) -> Vec<String> {
        vec![
            self.accent.clone(),
            mix(&self.ink, &self.paper, 0.62),
            mix(&self.accent, &self.paper, 0.5),
            mix(&self.ink, &self.paper, 0.3),
            self.accent2.clone(),
            mix(&self.ink, &self.paper, 0.8),
            mix(&self.accent2, &self.paper, 0.45),
        ]
    }

    /// The accent mixed toward the page: `amount` 0 = accent, 1 = paper.
    pub fn tint(&self, amount: f32) -> String {
        mix(&self.accent, &self.paper, amount)
    }

    pub fn serif_headings(&self) -> bool {
        self.heading_css.starts_with(SERIF_CSS.split(',').next().unwrap_or("'Source Serif 4'")) || self.heading_css.contains("Source Serif")
    }
}

/// The bundled typefaces (SIL Open Font License; see `fonts/`), embedded in
/// every PDF so it looks the same on any machine.
fn font_face_css() -> &'static str {
    static CSS: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    CSS.get_or_init(|| {
        use base64::Engine;
        let b64 = |b: &[u8]| base64::engine::general_purpose::STANDARD.encode(b);
        let face = |family: &str, style: &str, bytes: &[u8]| {
            format!("@font-face{{font-family:'{family}';font-style:{style};font-weight:100 900;font-display:block;src:url(data:font/woff2;base64,{}) format('woff2');}}", b64(bytes))
        };
        [
            face("Inter", "normal", include_bytes!("../../fonts/inter.woff2")),
            face("Inter", "italic", include_bytes!("../../fonts/inter-italic.woff2")),
            face("Source Serif 4", "normal", include_bytes!("../../fonts/source-serif-4.woff2")),
            face("Source Serif 4", "italic", include_bytes!("../../fonts/source-serif-4-italic.woff2")),
        ]
        .concat()
    })
}

/// `#4f46e5`, `4F46E5` or `#abc` → `4F46E5`.
pub fn hex(s: &str) -> Option<String> {
    let t = s.trim().trim_start_matches('#');
    let full = match t.len() {
        3 => t.chars().flat_map(|c| [c, c]).collect::<String>(),
        6 => t.to_string(),
        _ => return None,
    };
    full.chars().all(|c| c.is_ascii_hexdigit()).then(|| full.to_uppercase())
}

pub fn mix(a: &str, b: &str, amount: f32) -> String {
    let p = |s: &str, i: usize| u8::from_str_radix(s.get(i..i + 2).unwrap_or("00"), 16).unwrap_or(0) as f32;
    (0..3)
        .map(|k| {
            let v = p(a, k * 2) * (1.0 - amount) + p(b, k * 2) * amount;
            format!("{:02X}", v.round().clamp(0.0, 255.0) as u8)
        })
        .collect()
}

// ── Inline formatting ────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Span {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub code: bool,
    pub link: Option<String>,
}

/// `**bold**`, `*italic*` / `_italic_`, `` `code` `` and `[text](url)`.
/// Anything unmatched stays literal, so a stray asterisk is just text.
pub fn spans(s: &str) -> Vec<Span> {
    fn inner(s: &str, bold: bool, italic: bool, out: &mut Vec<Span>) {
        let chars: Vec<char> = s.chars().collect();
        let mut buf = String::new();
        let mut i = 0;
        let flush = |buf: &mut String, out: &mut Vec<Span>| {
            if !buf.is_empty() {
                out.push(Span { text: std::mem::take(buf), bold, italic, ..Default::default() });
            }
        };
        let find = |from: usize, pat: &[char]| -> Option<usize> {
            (from..chars.len().saturating_sub(pat.len() - 1)).find(|&j| chars[j..j + pat.len()] == *pat)
        };
        while i < chars.len() {
            let c = chars[i];
            if c == '`' {
                if let Some(end) = find(i + 1, &['`']) {
                    flush(&mut buf, out);
                    out.push(Span { text: chars[i + 1..end].iter().collect(), bold, italic, code: true, link: None });
                    i = end + 1;
                    continue;
                }
            }
            if c == '*' && chars.get(i + 1) == Some(&'*') {
                if let Some(end) = find(i + 2, &['*', '*']).filter(|&e| e > i + 2) {
                    flush(&mut buf, out);
                    inner(&chars[i + 2..end].iter().collect::<String>(), true, italic, out);
                    i = end + 2;
                    continue;
                }
            }
            if (c == '*' || c == '_') && chars.get(i + 1).is_some_and(|n| !n.is_whitespace()) {
                // `_` only at a word start, so snake_case stays literal.
                let word_start = i == 0 || !chars[i - 1].is_alphanumeric();
                if c == '*' || word_start {
                    if let Some(end) = find(i + 1, &[c]).filter(|&e| e > i + 1 && !chars[e - 1].is_whitespace()) {
                        flush(&mut buf, out);
                        inner(&chars[i + 1..end].iter().collect::<String>(), bold, true, out);
                        i = end + 1;
                        continue;
                    }
                }
            }
            if c == '[' {
                if let Some(close) = find(i + 1, &[']']) {
                    if chars.get(close + 1) == Some(&'(') {
                        if let Some(end) = find(close + 2, &[')']) {
                            flush(&mut buf, out);
                            out.push(Span {
                                text: chars[i + 1..close].iter().collect(),
                                bold,
                                italic,
                                code: false,
                                link: Some(chars[close + 2..end].iter().collect()),
                            });
                            i = end + 1;
                            continue;
                        }
                    }
                }
            }
            buf.push(c);
            i += 1;
        }
        flush(&mut buf, out);
    }
    let mut out = Vec::new();
    inner(s, false, false, &mut out);
    out
}

pub fn plain(s: &str) -> String {
    spans(s).into_iter().map(|s| s.text).collect()
}

// ── Content ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct Series {
    pub name: String,
    pub values: Vec<f64>,
}

#[derive(Debug, Clone)]
pub struct Chart {
    /// bar (vertical), hbar, line, area, pie, donut.
    pub kind: String,
    pub title: String,
    pub labels: Vec<String>,
    pub series: Vec<Series>,
    pub unit: String,
}

#[derive(Debug, Clone)]
pub struct Metric {
    pub value: String,
    pub label: String,
    pub note: String,
}

#[derive(Debug, Clone)]
pub struct Image {
    pub path: PathBuf,
    pub bytes: Vec<u8>,
    pub mime: &'static str,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone)]
pub enum Block {
    Heading { text: String, level: u8 },
    Paragraph { text: String },
    /// `(text, nesting level)`.
    List { items: Vec<(String, u8)>, ordered: bool },
    Table { rows: Vec<Vec<String>>, header: bool, caption: String, align: Vec<String> },
    Image { image: Image, caption: String, width_pct: u32 },
    Quote { text: String, by: String },
    Callout { tone: String, title: String, text: String },
    Code { text: String, language: String },
    Metrics { items: Vec<Metric> },
    Chart { chart: Chart },
    Divider,
    PageBreak,
}

pub fn cell_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        Value::Number(n) => {
            // 1234.5 → "1,234.5"; integers keep no decimals.
            n.as_f64().map(group_thousands).unwrap_or_else(|| n.to_string())
        }
        other => other.to_string(),
    }
}

fn group_thousands(f: f64) -> String {
    if !f.is_finite() || f.abs() >= 1e15 {
        return f.to_string();
    }
    let s = if f.fract() == 0.0 { format!("{}", f as i64) } else { format!("{f}") };
    let (int, frac) = s.split_once('.').map(|(a, b)| (a.to_string(), format!(".{b}"))).unwrap_or((s.clone(), String::new()));
    let (sign, digits) = int.strip_prefix('-').map(|d| ("-", d.to_string())).unwrap_or(("", int.clone()));
    let mut out = String::new();
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    format!("{sign}{out}{frac}")
}

fn str_of<'a>(b: &'a Value, k: &str) -> &'a str {
    b.get(k).and_then(|v| v.as_str()).unwrap_or("")
}

fn list_items(v: Option<&Value>, level: u8, out: &mut Vec<(String, u8)>) {
    for item in v.and_then(|v| v.as_array()).into_iter().flatten() {
        match item {
            Value::String(s) => out.push((s.clone(), level)),
            Value::Object(_) => {
                out.push((str_of(item, "text").to_string(), level));
                list_items(item.get("items"), (level + 1).min(2), out);
            }
            other => out.push((cell_text(other), level)),
        }
    }
}

pub fn load_image(cwd: &Path, rel: &str) -> Result<Image, String> {
    let p = Path::new(rel);
    let path = if p.is_absolute() { p.to_path_buf() } else { cwd.join(p) };
    let bytes = std::fs::read(&path).map_err(|e| format!("couldn't read image {}: {e}", path.display()))?;
    let mime = match path.extension().and_then(|e| e.to_str()).map(|e| e.to_lowercase()).as_deref() {
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        _ => return Err(format!("{}: images must be png, jpg, gif or webp", path.display())),
    };
    let (width, height) = image::ImageReader::new(std::io::Cursor::new(&bytes))
        .with_guessed_format()
        .ok()
        .and_then(|r| r.into_dimensions().ok())
        .ok_or_else(|| format!("couldn't read the size of {}", path.display()))?;
    Ok(Image { path, bytes, mime, width, height })
}

pub fn parse_chart(b: &Value) -> Result<Chart, String> {
    let labels: Vec<String> = b.get("labels").and_then(|l| l.as_array()).map(|a| a.iter().map(cell_text).collect()).unwrap_or_default();
    let mut series: Vec<Series> = b
        .get("series")
        .and_then(|s| s.as_array())
        .map(|a| {
            a.iter()
                .map(|s| Series {
                    name: str_of(s, "name").to_string(),
                    values: s.get("values").and_then(|v| v.as_array()).map(|v| v.iter().map(|x| x.as_f64().unwrap_or(0.0)).collect()).unwrap_or_default(),
                })
                .collect()
        })
        .unwrap_or_default();
    // Shorthand: `values` for a single series.
    if series.is_empty() {
        if let Some(v) = b.get("values").and_then(|v| v.as_array()) {
            series.push(Series { name: String::new(), values: v.iter().map(|x| x.as_f64().unwrap_or(0.0)).collect() });
        }
    }
    if series.is_empty() || labels.is_empty() {
        return Err("a chart needs `labels` and `series` ([{name, values}]) or `values`".into());
    }
    let kind = match str_of(b, "kind").to_lowercase().as_str() {
        "" | "bar" | "column" => "bar",
        "hbar" | "horizontal" | "barh" => "hbar",
        "line" => "line",
        "area" => "area",
        "pie" => "pie",
        "donut" | "doughnut" => "donut",
        other => return Err(format!("unknown chart kind '{other}': use bar, hbar, line, area, pie or donut")),
    }
    .to_string();
    Ok(Chart { kind, title: str_of(b, "title").to_string(), labels, series, unit: str_of(b, "unit").to_string() })
}

pub fn parse_blocks(v: Option<&Value>, cwd: &Path) -> Result<Vec<Block>, String> {
    let arr = v.and_then(|b| b.as_array()).ok_or("`blocks` must be an array")?;
    let mut out = Vec::new();
    for (i, b) in arr.iter().enumerate() {
        let kind = str_of(b, "type");
        let text = str_of(b, "text").to_string();
        let at = |e: String| format!("block {i} ({kind}): {e}");
        out.push(match kind {
            "heading" => Block::Heading { text, level: b.get("level").and_then(|l| l.as_u64()).unwrap_or(1).clamp(1, 3) as u8 },
            "" | "paragraph" | "text" => Block::Paragraph { text },
            "bullets" | "list" | "numbered" => {
                let mut items = Vec::new();
                list_items(b.get("items"), 0, &mut items);
                Block::List { items, ordered: kind == "numbered" || b.get("ordered").and_then(|o| o.as_bool()).unwrap_or(false) }
            }
            "table" => Block::Table {
                rows: b
                    .get("rows")
                    .and_then(|r| r.as_array())
                    .map(|a| a.iter().map(|row| row.as_array().map(|c| c.iter().map(cell_text).collect()).unwrap_or_default()).collect())
                    .unwrap_or_default(),
                header: b.get("header").and_then(|h| h.as_bool()).unwrap_or(true),
                caption: str_of(b, "caption").to_string(),
                align: b.get("align").and_then(|a| a.as_array()).map(|a| a.iter().map(cell_text).collect()).unwrap_or_default(),
            },
            "image" => Block::Image {
                image: load_image(cwd, str_of(b, "path")).map_err(at)?,
                caption: str_of(b, "caption").to_string(),
                width_pct: b.get("width").and_then(|w| w.as_u64()).unwrap_or(100).clamp(10, 100) as u32,
            },
            "quote" => Block::Quote { text, by: str_of(b, "by").to_string() },
            "callout" | "note" => Block::Callout {
                tone: match str_of(b, "tone") {
                    t @ ("success" | "warning" | "danger") => t.to_string(),
                    _ => "info".to_string(),
                },
                title: str_of(b, "title").to_string(),
                text,
            },
            "code" => Block::Code { text, language: str_of(b, "language").to_string() },
            "metrics" | "stats" => Block::Metrics {
                items: b
                    .get("items")
                    .and_then(|a| a.as_array())
                    .map(|a| {
                        a.iter()
                            .map(|m| Metric { value: cell_text(m.get("value").unwrap_or(&Value::Null)), label: str_of(m, "label").into(), note: str_of(m, "note").into() })
                            .collect()
                    })
                    .unwrap_or_default(),
            },
            "chart" => Block::Chart { chart: parse_chart(b).map_err(at)? },
            "divider" | "rule" => Block::Divider,
            "page_break" | "pagebreak" => Block::PageBreak,
            other => {
                return Err(format!(
                    "block {i} has unknown type '{other}': use heading, paragraph, bullets, numbered, table, image, quote, callout, code, metrics, chart, divider or page_break"
                ))
            }
        });
    }
    Ok(out)
}

/// Title, cover and page settings shared by PDF and Word.
#[derive(Debug, Clone, Default)]
pub struct DocMeta {
    pub title: String,
    pub subtitle: String,
    pub author: String,
    pub date: String,
    pub cover: bool,
    pub letter: bool,
    pub landscape: bool,
    pub footer: String,
    /// A short label above the title ("Quarterly report", "Proposal").
    pub eyebrow: String,
    /// The page in millimetres, before orientation: A4 unless set.
    pub page: Option<(f32, f32)>,
}

impl DocMeta {
    pub fn from_args(args: &Value) -> DocMeta {
        DocMeta {
            title: str_of(args, "title").into(),
            subtitle: str_of(args, "subtitle").into(),
            author: str_of(args, "author").into(),
            date: str_of(args, "date").into(),
            cover: args.get("cover").and_then(|c| c.as_bool()).unwrap_or(false),
            letter: str_of(args, "page_size").eq_ignore_ascii_case("letter"),
            landscape: str_of(args, "orientation").eq_ignore_ascii_case("landscape"),
            footer: str_of(args, "footer").into(),
            eyebrow: str_of(args, "eyebrow").into(),
            page: args
                .get("page")
                .and_then(|p| Some((p.get("width_mm")?.as_f64()? as f32, p.get("height_mm")?.as_f64()? as f32)))
                .filter(|(w, h)| *w >= 20.0 && *h >= 20.0 && *w <= 2000.0 && *h <= 2000.0)
                .or_else(|| match str_of(args, "page_size").to_lowercase().as_str() {
                    "a3" => Some((297.0, 420.0)),
                    "a5" => Some((148.0, 210.0)),
                    "legal" => Some((215.9, 355.6)),
                    _ => None,
                }),
        }
    }
}

// ── HTML ─────────────────────────────────────────────────────────────────

pub fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

pub fn inline_html(s: &str) -> String {
    spans(s)
        .into_iter()
        .map(|sp| {
            let mut t = esc(&sp.text).replace('\n', "<br>");
            if sp.code {
                t = format!("<code>{t}</code>");
            }
            if sp.italic {
                t = format!("<em>{t}</em>");
            }
            if sp.bold {
                t = format!("<strong>{t}</strong>");
            }
            if let Some(url) = sp.link {
                t = format!("<a href=\"{}\">{t}</a>", esc(&url));
            }
            t
        })
        .collect()
}

fn data_uri(img: &Image) -> String {
    use base64::Engine;
    format!("data:{};base64,{}", img.mime, base64::engine::general_purpose::STANDARD.encode(&img.bytes))
}

/// The colour a callout's label takes: the only place tone shows.
pub fn tone_color(tone: &str) -> &'static str {
    match tone {
        "success" => "2F7D4F",
        "warning" => "A1590B",
        "danger" => "B42318",
        _ => "3451D1",
    }
}

pub fn looks_numeric(s: &str) -> bool {
    let t = s.trim().trim_start_matches(['$', '€', '£', '¥', '₹', '+', '-', '−', '(']).trim_end_matches(['%', 'x', 'k', 'K', 'M', 'B', ')']);
    !t.is_empty() && t.chars().all(|c| c.is_ascii_digit() || c == ',' || c == '.') && t.chars().any(|c| c.is_ascii_digit())
}

/// Per column: right-aligned when its body cells are mostly numbers, so the
/// header sits over its figures.
pub fn column_align(rows: &[Vec<String>], header: bool, explicit: &[String]) -> Vec<&'static str> {
    let cols = rows.iter().map(|r| r.len()).max().unwrap_or(0);
    (0..cols)
        .map(|c| match explicit.get(c).map(|s| s.as_str()) {
            Some("right") => "right",
            Some("center") => "center",
            Some("left") => "left",
            _ => {
                let body: Vec<&String> = rows.iter().skip(if header { 1 } else { 0 }).filter_map(|r| r.get(c)).filter(|s| !s.trim().is_empty()).collect();
                let numeric = body.iter().filter(|s| looks_numeric(s)).count();
                if c > 0 && !body.is_empty() && numeric * 2 >= body.len() { "right" } else { "left" }
            }
        })
        .collect()
}

fn table_html(rows: &[Vec<String>], header: bool, caption: &str, align: &[String]) -> String {
    let aligns = column_align(rows, header, align);
    let mut h = String::from("<table>");
    for (ri, row) in rows.iter().enumerate() {
        let head = header && ri == 0;
        h.push_str(if head { "<thead><tr>" } else { "<tr>" });
        for (ci, cell) in row.iter().enumerate() {
            let tag = if head { "th" } else { "td" };
            let a = aligns.get(ci).copied().unwrap_or("left");
            h.push_str(&format!("<{tag} class=\"{a}\">{}</{tag}>", inline_html(cell)));
        }
        h.push_str(if head { "</tr></thead><tbody>" } else { "</tr>" });
    }
    h.push_str("</tbody>");
    if !caption.is_empty() {
        h.push_str(&format!("<caption>{}</caption>", inline_html(caption)));
    }
    h.push_str("</table>");
    h
}

fn list_html(items: &[(String, u8)], ordered: bool) -> String {
    let tag = if ordered { "ol" } else { "ul" };
    let mut h = format!("<{tag}>");
    let mut level = 0u8;
    for (i, (text, l)) in items.iter().enumerate() {
        while level < *l {
            h.push_str(&format!("<{tag}>"));
            level += 1;
        }
        while level > *l {
            h.push_str(&format!("</li></{tag}>"));
            level -= 1;
        }
        if i > 0 && items[i - 1].1 >= *l {
            h.push_str("</li>");
        }
        h.push_str(&format!("<li>{}", inline_html(text)));
    }
    if !items.is_empty() {
        h.push_str("</li>");
    }
    while level > 0 {
        h.push_str(&format!("</{tag}></li>"));
        level -= 1;
    }
    h.push_str(&format!("</{tag}>"));
    h
}

fn metrics_html(items: &[Metric]) -> String {
    let mut h = format!("<div class=\"metrics\" style=\"grid-template-columns:repeat({},1fr)\">", items.len().clamp(1, 4));
    for m in items {
        h.push_str(&format!(
            "<div class=\"metric\"><div class=\"metric-label\">{}</div><div class=\"metric-value\">{}</div>{}</div>",
            inline_html(&m.label),
            inline_html(&m.value),
            if m.note.is_empty() { String::new() } else { format!("<div class=\"metric-note\">{}</div>", inline_html(&m.note)) }
        ));
    }
    h.push_str("</div>");
    h
}

fn blocks_html(blocks: &[Block], theme: &Theme) -> String {
    let mut h = String::new();
    for b in blocks {
        match b {
            Block::Heading { text, level } => h.push_str(&format!("<h{l}>{}</h{l}>", inline_html(text), l = level + 1)),
            Block::Paragraph { text } => h.push_str(&format!("<p>{}</p>", inline_html(text))),
            Block::List { items, ordered } => h.push_str(&list_html(items, *ordered)),
            Block::Table { rows, header, caption, align } => h.push_str(&table_html(rows, *header, caption, align)),
            Block::Image { image, caption, width_pct } => {
                h.push_str(&format!("<figure><img src=\"{}\" style=\"width:{width_pct}%\">", data_uri(image)));
                if !caption.is_empty() {
                    h.push_str(&format!("<figcaption>{}</figcaption>", inline_html(caption)));
                }
                h.push_str("</figure>");
            }
            Block::Quote { text, by } => {
                h.push_str(&format!("<blockquote><p>{}</p>", inline_html(text)));
                if !by.is_empty() {
                    h.push_str(&format!("<cite>{}</cite>", inline_html(by)));
                }
                h.push_str("</blockquote>");
            }
            Block::Callout { tone, title, text } => {
                h.push_str("<aside class=\"callout\">");
                if !title.is_empty() {
                    h.push_str(&format!("<div class=\"callout-title\" style=\"color:#{}\">{}</div>", tone_color(tone), inline_html(title)));
                }
                h.push_str(&format!("<div>{}</div></aside>", inline_html(text)));
            }
            Block::Code { text, .. } => h.push_str(&format!("<pre><code>{}</code></pre>", esc(text))),
            Block::Metrics { items } => h.push_str(&metrics_html(items)),
            Block::Chart { chart } => {
                h.push_str("<figure class=\"chart\">");
                if !chart.title.is_empty() {
                    h.push_str(&format!("<div class=\"chart-title\">{}</div>", esc(&chart.title)));
                }
                h.push_str(&chart_svg(chart, theme, 640, 260));
                h.push_str("</figure>");
            }
            Block::Divider => h.push_str("<hr>"),
            Block::PageBreak => h.push_str("<div class=\"page-break\"></div>"),
        }
    }
    h
}

pub fn page_mm(meta: &DocMeta) -> (f32, f32) {
    let (w, h) = meta.page.unwrap_or(if meta.letter { (215.9, 279.4) } else { (210.0, 297.0) });
    if meta.landscape { (h, w) } else { (w, h) }
}

/// Text for CSS `content: "…"`.
fn css_str(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', " ")
}

pub fn base_css(theme: &Theme, meta: &DocMeta) -> String {
    let (w, h) = page_mm(meta);
    let size = format!("{w}mm {h}mm");
    let t = theme;
    let running = if meta.footer.is_empty() { meta.title.clone() } else { meta.footer.clone() };
    format!(
        r#"{fonts}
@page {{ size: {size}; margin: 21mm 24mm 22mm;
  @top-left {{ content: "{running}"; font: 600 6.6pt/1 {body}; letter-spacing: .11em; text-transform: uppercase; color: #{faint}; vertical-align: bottom; padding-bottom: 6mm; }}
  @bottom-right {{ content: counter(page); font: 500 7.5pt/1 {body}; color: #{muted}; vertical-align: top; padding-top: 6mm; }} }}
@page :first {{ @top-left {{ content: none; }} {first_bottom} }}
* {{ box-sizing: border-box; }}
html {{ -webkit-print-color-adjust: exact; print-color-adjust: exact; background: #{paper}; }}
body {{ margin: 0; font: 400 10pt/1.62 {body}; color: #{ink}; background: #{paper}; font-optical-sizing: auto; font-kerning: normal; text-rendering: optimizeLegibility; font-feature-settings: "kern", "liga", "calt", "ss01"; hyphens: auto; }}
@media screen {{ body {{ width: {w}mm; min-height: {h}mm; padding: 21mm 24mm 22mm; }} }}
.masthead {{ margin: 0 0 24pt; }}
.eyebrow {{ font: 650 7pt/1 {body}; letter-spacing: .14em; text-transform: uppercase; color: #{accent}; margin: 0 0 11pt; }}
.doc-title {{ font: 650 26pt/1.1 {heading}; letter-spacing: -0.024em; color: #{ink}; margin: 0; text-wrap: balance; }}
.doc-subtitle {{ font: 400 12.5pt/1.45 {body}; color: #{muted}; margin: 8pt 0 0; max-width: 88%; text-wrap: balance; }}
.doc-meta {{ margin-top: 16pt; padding-top: 8pt; border-top: 0.6pt solid #{line}; font: 500 7.8pt/1.4 {body}; color: #{muted}; display: flex; gap: 18pt; }}
h2, h3, h4 {{ break-after: avoid; text-wrap: balance; }}
h2 {{ font: 640 14pt/1.25 {heading}; letter-spacing: -0.014em; color: #{ink}; margin: 26pt 0 8pt; }}
h3 {{ font: 620 11pt/1.3 {heading}; letter-spacing: -0.006em; color: #{ink}; margin: 18pt 0 5pt; }}
h4 {{ font: 650 7pt/1.3 {body}; letter-spacing: .12em; text-transform: uppercase; color: #{muted}; margin: 16pt 0 6pt; }}
.masthead + h2, .cover + h2 {{ margin-top: 0; }}
p {{ margin: 0 0 8pt; text-wrap: pretty; orphans: 3; widows: 3; }}
a {{ color: inherit; text-decoration: underline; text-decoration-color: #{accent}; text-decoration-thickness: 0.7pt; text-underline-offset: 2pt; }}
strong {{ font-weight: 640; }}
ul, ol {{ margin: 0 0 9pt; padding-left: 15pt; }}
ul {{ list-style-type: "–  "; }} ul ul {{ list-style-type: "·  "; }}
li {{ margin: 2.4pt 0; padding-left: 1pt; }}
li::marker {{ color: #{faint}; }}
ol > li::marker {{ color: #{muted}; font-weight: 550; font-variant-numeric: tabular-nums; }}
code {{ font: 0.88em/1 {mono}; background: #{surface}; padding: 0.6pt 3pt; border-radius: 2.5pt; }}
pre {{ background: #{surface}; border-radius: 4pt; padding: 10pt 12pt; font: 8.2pt/1.6 {mono}; white-space: pre-wrap; margin: 4pt 0 13pt; break-inside: avoid; }}
pre code {{ background: none; padding: 0; font-size: 1em; }}
table {{ width: 100%; border-collapse: collapse; margin: 6pt 0 15pt; font-size: 8.8pt; line-height: 1.45; font-variant-numeric: tabular-nums lining-nums; }}
thead th {{ font: 650 6.8pt/1.2 {body}; letter-spacing: .1em; text-transform: uppercase; color: #{muted}; padding: 0 9pt 6pt; border-bottom: 0.9pt solid #{ink}; vertical-align: bottom; }}
td {{ padding: 6pt 9pt; border-bottom: 0.5pt solid #{line}; vertical-align: top; }}
th:first-child, td:first-child {{ padding-left: 0; }} th:last-child, td:last-child {{ padding-right: 0; }}
.left {{ text-align: left; }} .right {{ text-align: right; }} .center {{ text-align: center; }}
thead {{ display: table-header-group; }} tr {{ break-inside: avoid; }}
caption {{ caption-side: bottom; text-align: left; font-size: 7.6pt; color: #{muted}; padding-top: 6pt; }}
figure {{ margin: 10pt 0 16pt; break-inside: avoid; }}
figure img {{ display: block; max-width: 100%; margin: 0 auto; }}
figcaption {{ font-size: 7.6pt; color: #{muted}; margin-top: 7pt; }}
blockquote {{ margin: 18pt 0 20pt; padding: 0 0 0 0.45em; break-inside: avoid; }}
blockquote p {{ font: italic 400 15pt/1.38 {serif}; letter-spacing: -0.006em; color: #{ink}; margin: 0; text-indent: -0.45em; text-wrap: balance; }}
blockquote p::before {{ content: "“"; color: #{accent}; }} blockquote p::after {{ content: "”"; color: #{accent}; }}
blockquote cite {{ display: block; margin-top: 8pt; font: 600 6.8pt/1.3 {body}; letter-spacing: .12em; text-transform: uppercase; font-style: normal; color: #{muted}; }}
.callout {{ background: #{surface}; border-radius: 4pt; padding: 10pt 13pt 11pt; margin: 6pt 0 14pt; break-inside: avoid; }}
.callout-title {{ font: 650 6.8pt/1.2 {body}; letter-spacing: .12em; text-transform: uppercase; margin-bottom: 4pt; }}
.callout > div:last-child {{ margin: 0; }}
.metrics {{ display: grid; margin: 12pt 0 20pt; border-top: 0.9pt solid #{ink}; break-inside: avoid; }}
.metric {{ padding: 9pt 14pt 0 0; }} .metric + .metric {{ padding-left: 14pt; border-left: 0.5pt solid #{line}; }}
.metric-label {{ font: 650 6.6pt/1.2 {body}; letter-spacing: .12em; text-transform: uppercase; color: #{muted}; }}
.metric-value {{ font: 560 21pt/1.1 {heading_num}; letter-spacing: -0.025em; margin-top: 7pt; font-variant-numeric: lining-nums tabular-nums; }}
.metric-note {{ font-size: 7.8pt; color: #{muted}; margin-top: 3pt; }}
.chart-title {{ font: 620 9pt/1.3 {body}; margin-bottom: 4pt; }}
.chart svg {{ display: block; width: 100%; height: auto; }}
hr {{ border: 0; border-top: 0.5pt solid #{line}; margin: 22pt 0; }}
.page-break {{ break-after: page; }}
.cover {{ height: {cover_h}mm; display: flex; flex-direction: column; break-after: page; }}
.cover-top {{ display: flex; justify-content: space-between; font: 600 6.8pt/1 {body}; letter-spacing: .13em; text-transform: uppercase; color: #{muted}; }}
.cover-main {{ margin: auto 0; padding-bottom: 14%; }}
.cover .doc-title {{ font-size: 38pt; line-height: 1.04; letter-spacing: -0.03em; max-width: 92%; }}
.cover .doc-subtitle {{ font-size: 14.5pt; margin-top: 14pt; max-width: 80%; }}
.cover-bottom {{ border-top: 0.6pt solid #{line}; padding-top: 8pt; display: flex; justify-content: space-between; font: 500 7.8pt/1.4 {body}; color: #{muted}; }}
"#,
        fonts = font_face_css(),
        size = size,
        running = css_str(&running),
        first_bottom = if meta.cover { "@bottom-right { content: none; }" } else { "" },
        body = t.body_css,
        heading = t.heading_css,
        // Big numbers read best in the sans, even under serif headings.
        heading_num = if t.serif_headings() { SANS_CSS } else { t.heading_css.as_str() },
        serif = SERIF_CSS,
        mono = MONO_CSS,
        ink = t.ink,
        muted = t.muted,
        faint = t.faint,
        paper = t.paper,
        accent = t.accent,
        surface = t.surface,
        line = t.line,
        w = w,
        h = h,
        cover_h = h - 43.5,
    )
}

pub fn document_html(meta: &DocMeta, blocks: &[Block], theme: &Theme) -> String {
    let mut head = String::new();
    let byline: Vec<String> = [meta.author.as_str(), meta.date.as_str()].iter().filter(|s| !s.is_empty()).map(|s| esc(s)).collect();
    if meta.cover {
        head.push_str("<section class=\"cover\">");
        head.push_str(&format!(
            "<div class=\"cover-top\"><span>{}</span><span>{}</span></div>",
            esc(if meta.eyebrow.is_empty() { &meta.author } else { &meta.eyebrow }),
            esc(&meta.date)
        ));
        head.push_str(&format!("<div class=\"cover-main\"><div class=\"doc-title\">{}</div>", inline_html(&meta.title)));
        if !meta.subtitle.is_empty() {
            head.push_str(&format!("<div class=\"doc-subtitle\">{}</div>", inline_html(&meta.subtitle)));
        }
        head.push_str("</div>");
        head.push_str(&format!(
            "<div class=\"cover-bottom\"><span>{}</span><span>{}</span></div></section>",
            esc(if meta.eyebrow.is_empty() { &meta.date } else { &meta.author }),
            esc(&meta.footer)
        ));
    } else if !meta.title.is_empty() {
        head.push_str("<header class=\"masthead\">");
        if !meta.eyebrow.is_empty() {
            head.push_str(&format!("<div class=\"eyebrow\">{}</div>", esc(&meta.eyebrow)));
        }
        head.push_str(&format!("<div class=\"doc-title\">{}</div>", inline_html(&meta.title)));
        if !meta.subtitle.is_empty() {
            head.push_str(&format!("<div class=\"doc-subtitle\">{}</div>", inline_html(&meta.subtitle)));
        }
        if !byline.is_empty() {
            head.push_str(&format!("<div class=\"doc-meta\">{}</div>", byline.iter().map(|b| format!("<span>{b}</span>")).collect::<String>()));
        }
        head.push_str("</header>");
    }
    format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><title>{}</title><style>{}</style></head><body>{}{}</body></html>",
        esc(&meta.title),
        base_css(theme, meta),
        head,
        blocks_html(blocks, theme)
    )
}

/// A free-form graphic's HTML: the agent's page on a canvas of exactly
/// `w`×`h` CSS pixels, with the bundled fonts available.
pub fn canvas_html(html: &str, w: u32, h: u32) -> String {
    let t = html.trim_start().to_lowercase();
    if t.starts_with("<!doctype") || t.starts_with("<html") {
        return html.replacen("<head>", &format!("<head><style>{}</style>", font_face_css()), 1);
    }
    format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><style>{}*{{box-sizing:border-box}}html,body{{margin:0;width:{w}px;height:{h}px;overflow:hidden}}body{{font-family:{SANS_CSS};font-optical-sizing:auto;font-kerning:normal}}</style></head><body>{html}</body></html>",
        font_face_css()
    )
}

/// The agent's own HTML: a full document is used as is; a fragment gets the
/// theme's stylesheet (fonts, type scale, tables, `.callout`, `.metrics`,
/// `.eyebrow`…), so it looks like everything else.
pub fn custom_html(html: &str, meta: &DocMeta, theme: &Theme) -> String {
    let t = html.trim_start().to_lowercase();
    if t.starts_with("<!doctype") || t.starts_with("<html") {
        // Still offer the bundled fonts to a full document.
        return html.replacen("<head>", &format!("<head><style>{}</style>", font_face_css()), 1);
    }
    format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><title>{}</title><style>{}</style></head><body>{html}</body></html>",
        esc(&meta.title),
        base_css(theme, meta)
    )
}

// ── Slides as HTML (PDF decks, and slide previews) ───────────────────────

#[derive(Debug, Clone)]
pub struct Slide {
    pub layout: String,
    pub title: String,
    pub subtitle: String,
    pub bullets: Vec<(String, u8)>,
    pub left: (String, Vec<(String, u8)>),
    pub right: (String, Vec<(String, u8)>),
    pub image: Option<Image>,
    pub caption: String,
    pub quote: String,
    pub by: String,
    pub stats: Vec<Metric>,
    pub table: Vec<Vec<String>>,
    pub chart: Option<Chart>,
    pub notes: String,
}

pub fn parse_slides(v: &Value, cwd: &Path) -> Result<Vec<Slide>, String> {
    let arr = v.as_array().ok_or("`slides` must be an array")?;
    if arr.is_empty() {
        return Err("`slides` was empty".into());
    }
    let column = |c: Option<&Value>| -> (String, Vec<(String, u8)>) {
        let mut items = Vec::new();
        let heading = c.map(|c| str_of(c, "heading").to_string()).unwrap_or_default();
        list_items(c.and_then(|c| c.get("bullets")), 0, &mut items);
        (heading, items)
    };
    arr.iter()
        .enumerate()
        .map(|(i, s)| {
            let at = |e: String| format!("slide {}: {e}", i + 1);
            let mut bullets = Vec::new();
            list_items(s.get("bullets"), 0, &mut bullets);
            let image = match s.get("image").and_then(|v| v.as_str()) {
                Some(p) if !p.is_empty() => Some(load_image(cwd, p).map_err(at)?),
                _ => None,
            };
            let chart = match s.get("chart") {
                Some(c) if c.is_object() => Some(parse_chart(c).map_err(at)?),
                _ => None,
            };
            let table: Vec<Vec<String>> = s
                .get("table")
                .and_then(|t| t.get("rows").or(Some(t)))
                .and_then(|r| r.as_array())
                .map(|a| a.iter().map(|row| row.as_array().map(|c| c.iter().map(cell_text).collect()).unwrap_or_default()).collect())
                .unwrap_or_default();
            let stats: Vec<Metric> = s
                .get("stats")
                .and_then(|a| a.as_array())
                .map(|a| a.iter().map(|m| Metric { value: cell_text(m.get("value").unwrap_or(&Value::Null)), label: str_of(m, "label").into(), note: str_of(m, "note").into() }).collect())
                .unwrap_or_default();
            let mut slide = Slide {
                layout: str_of(s, "layout").to_lowercase(),
                title: str_of(s, "title").into(),
                subtitle: str_of(s, "subtitle").into(),
                bullets,
                left: column(s.get("left")),
                right: column(s.get("right")),
                image,
                caption: str_of(s, "caption").into(),
                quote: str_of(s, "quote").into(),
                by: str_of(s, "by").into(),
                stats,
                table,
                chart,
                notes: str_of(s, "notes").into(),
            };
            if slide.layout.is_empty() || slide.layout == "auto" {
                slide.layout = if i == 0 && slide.bullets.is_empty() && slide.chart.is_none() && slide.table.is_empty() {
                    "title"
                } else if !slide.quote.is_empty() {
                    "quote"
                } else if !slide.stats.is_empty() {
                    "stats"
                } else if slide.chart.is_some() {
                    "chart"
                } else if !slide.table.is_empty() {
                    "table"
                } else if !slide.left.1.is_empty() || !slide.right.1.is_empty() {
                    "two_column"
                } else if slide.image.is_some() {
                    "image"
                } else if slide.bullets.is_empty() && !slide.subtitle.is_empty() {
                    "section"
                } else {
                    "bullets"
                }
                .to_string();
            }
            Ok(slide)
        })
        .collect()
}

/// Slide points: separated by hairlines rather than bullets, sub-points
/// quieter beneath.
fn points_html(items: &[(String, u8)]) -> String {
    if items.is_empty() {
        return String::new();
    }
    let mut h = String::from("<ul class=\"points\">");
    let mut open = false;
    for (text, l) in items {
        if *l == 0 {
            if open {
                h.push_str("</li>");
            }
            h.push_str(&format!("<li>{}", inline_html(text)));
            open = true;
        } else {
            h.push_str(&format!("<div class=\"sub\">{}</div>", inline_html(text)));
        }
    }
    if open {
        h.push_str("</li>");
    }
    h.push_str("</ul>");
    h
}

/// The deck's name, for slide footers: its title slide's title.
pub fn deck_name(slides: &[Slide]) -> String {
    slides.iter().find(|s| s.layout == "title").or(slides.first()).map(|s| plain(&s.title)).unwrap_or_default()
}

pub fn deck_html(slides: &[Slide], theme: &Theme) -> String {
    let t = theme;
    let css = format!(
        r#"{fonts}
@page {{ size: 338.67mm 190.5mm; margin: 0; }}
* {{ box-sizing: border-box; }}
html {{ -webkit-print-color-adjust: exact; print-color-adjust: exact; }}
body {{ margin: 0; font-family: {body}; color: #{ink}; background: #{paper}; font-optical-sizing: auto; font-kerning: normal; font-feature-settings: "kern", "liga", "calt", "ss01"; }}
.slide {{ width: 338.67mm; height: 190.5mm; position: relative; overflow: hidden; break-after: page; background: #{paper}; padding: 16mm 20mm 18mm; display: flex; flex-direction: column; }}
.slide h1 {{ font: 640 27pt/1.12 {heading}; letter-spacing: -0.024em; margin: 0 0 10mm; max-width: 82%; text-wrap: balance; }}
.lede {{ font: 400 15pt/1.4 {body}; color: #{muted}; margin: -6mm 0 9mm; max-width: 72%; text-wrap: balance; }}
.foot {{ position: absolute; left: 20mm; right: 20mm; bottom: 8mm; display: flex; justify-content: space-between; font: 600 7pt/1 {body}; letter-spacing: .13em; text-transform: uppercase; color: #{faint}; }}
.points {{ list-style: none; margin: 0; padding: 0; max-width: 78%; }}
.points > li {{ font: 400 19pt/1.32 {body}; padding: 4mm 0; border-top: 0.6pt solid #{line}; text-wrap: pretty; }}
.points > li:last-child {{ border-bottom: 0.6pt solid #{line}; }}
.points .sub {{ font-size: 13.5pt; color: #{muted}; margin-top: 1.5mm; }}
.hero {{ justify-content: flex-end; padding-bottom: 26mm; }}
.hero .mark {{ width: 12mm; height: 1.4pt; background: #{accent}; margin-bottom: 8mm; }}
.hero h1 {{ font-size: 46pt; line-height: 1.04; letter-spacing: -0.032em; margin: 0; max-width: 84%; }}
.hero .sub {{ font: 400 17pt/1.4 {body}; color: #{muted}; margin-top: 6mm; max-width: 70%; }}
.section {{ background: #{surface}; justify-content: center; }}
.section .num {{ font: 300 58pt/1 {numfont}; letter-spacing: -0.04em; color: #{accent}; margin-bottom: 6mm; font-variant-numeric: lining-nums; }}
.section h1 {{ font-size: 38pt; margin: 0; }}
.section .sub {{ font: 400 16pt/1.4 {body}; color: #{muted}; margin-top: 5mm; max-width: 65%; }}
.cols {{ display: grid; grid-template-columns: 1fr 1fr; gap: 16mm; }}
.cols h3 {{ font: 650 8pt/1.2 {body}; letter-spacing: .13em; text-transform: uppercase; color: #{accent}; margin: 0 0 1mm; }}
.cols .points {{ max-width: none; }}
.media {{ display: grid; grid-template-columns: 1.35fr 1fr; gap: 14mm; flex: 1; min-height: 0; align-items: center; }}
.media.solo {{ grid-template-columns: 1fr; }}
.media img {{ width: 100%; max-height: 118mm; object-fit: contain; }}
.media .caption {{ font: 400 12pt/1.45 {body}; color: #{muted}; margin-top: 5mm; }}
.quote {{ justify-content: center; }}
.quote blockquote {{ margin: 0; max-width: 84%; }}
.quote blockquote p {{ font: italic 400 33pt/1.24 {serif}; letter-spacing: -0.012em; margin: 0; text-indent: -0.42em; text-wrap: balance; }}
.quote blockquote p::before {{ content: "“"; color: #{accent}; }} .quote blockquote p::after {{ content: "”"; color: #{accent}; }}
.quote cite {{ display: block; margin-top: 9mm; font: 600 9pt/1.3 {body}; letter-spacing: .13em; text-transform: uppercase; font-style: normal; color: #{muted}; }}
.stats {{ display: grid; margin: auto 0; border-top: 0.9pt solid #{ink}; }}
.stat {{ padding: 7mm 10mm 0 0; }} .stat + .stat {{ padding-left: 10mm; border-left: 0.6pt solid #{line}; }}
.stat .l {{ font: 650 8pt/1.2 {body}; letter-spacing: .13em; text-transform: uppercase; color: #{muted}; }}
.stat .v {{ font: 300 54pt/1 {numfont}; letter-spacing: -0.035em; margin-top: 5mm; font-variant-numeric: lining-nums tabular-nums; }}
.stat .n {{ font: 400 12pt/1.4 {body}; color: #{muted}; margin-top: 3mm; }}
table {{ width: 100%; border-collapse: collapse; font-size: 13pt; line-height: 1.4; font-variant-numeric: tabular-nums lining-nums; }}
thead th {{ font: 650 8pt/1.2 {body}; letter-spacing: .12em; text-transform: uppercase; color: #{muted}; padding: 0 5mm 3mm; border-bottom: 0.9pt solid #{ink}; }}
td {{ padding: 3.2mm 5mm; border-bottom: 0.6pt solid #{line}; }}
th:first-child, td:first-child {{ padding-left: 0; }} th:last-child, td:last-child {{ padding-right: 0; }}
.left {{ text-align: left; }} .right {{ text-align: right; }} .center {{ text-align: center; }}
.chart {{ flex: 1; min-height: 0; display: flex; align-items: center; }} .chart svg {{ width: 100%; max-height: 118mm; }}
"#,
        fonts = font_face_css(),
        body = t.body_css,
        heading = t.heading_css,
        numfont = if t.serif_headings() { SANS_CSS } else { t.heading_css.as_str() },
        serif = SERIF_CSS,
        ink = t.ink,
        muted = t.muted,
        faint = t.faint,
        paper = t.paper,
        surface = t.surface,
        line = t.line,
        accent = t.accent,
    );
    let name = esc(&deck_name(slides));
    let mut body = String::new();
    let mut section = 0;
    for (i, s) in slides.iter().enumerate() {
        let foot = format!("<div class=\"foot\"><span>{name}</span><span>{}</span></div>", i + 1);
        let title = if s.title.is_empty() { String::new() } else { format!("<h1>{}</h1>", inline_html(&s.title)) };
        let lede = if s.subtitle.is_empty() { String::new() } else { format!("<div class=\"lede\">{}</div>", inline_html(&s.subtitle)) };
        let html = match s.layout.as_str() {
            "title" | "closing" => format!(
                "<section class=\"slide hero\"><div class=\"mark\"></div>{title}{}</section>",
                if s.subtitle.is_empty() { String::new() } else { format!("<div class=\"sub\">{}</div>", inline_html(&s.subtitle)) }
            ),
            "section" => {
                section += 1;
                format!(
                    "<section class=\"slide section\"><div class=\"num\">{section:02}</div>{title}{}{foot}</section>",
                    if s.subtitle.is_empty() { String::new() } else { format!("<div class=\"sub\">{}</div>", inline_html(&s.subtitle)) }
                )
            }
            "two_column" => format!(
                "<section class=\"slide\">{title}{lede}<div class=\"cols\"><div><h3>{}</h3>{}</div><div><h3>{}</h3>{}</div></div>{foot}</section>",
                inline_html(&s.left.0),
                points_html(&s.left.1),
                inline_html(&s.right.0),
                points_html(&s.right.1)
            ),
            "image" => {
                let side = !s.bullets.is_empty() || !s.caption.is_empty();
                format!(
                    "<section class=\"slide\">{title}<div class=\"media{}\">{}{}</div>{foot}</section>",
                    if side { "" } else { " solo" },
                    s.image.as_ref().map(|im| format!("<img src=\"{}\">", data_uri(im))).unwrap_or_default(),
                    if side { format!("<div>{}<div class=\"caption\">{}</div></div>", points_html(&s.bullets), inline_html(&s.caption)) } else { String::new() }
                )
            }
            "quote" => format!(
                "<section class=\"slide quote\"><blockquote><p>{}</p>{}</blockquote>{foot}</section>",
                inline_html(&s.quote),
                if s.by.is_empty() { String::new() } else { format!("<cite>{}</cite>", inline_html(&s.by)) }
            ),
            "stats" => format!(
                "<section class=\"slide\">{title}{lede}<div class=\"stats\" style=\"grid-template-columns:repeat({},1fr)\">{}</div>{foot}</section>",
                s.stats.len().clamp(1, 4),
                s.stats
                    .iter()
                    .take(4)
                    .map(|m| format!(
                        "<div class=\"stat\"><div class=\"l\">{}</div><div class=\"v\">{}</div>{}</div>",
                        inline_html(&m.label),
                        inline_html(&m.value),
                        if m.note.is_empty() { String::new() } else { format!("<div class=\"n\">{}</div>", inline_html(&m.note)) }
                    ))
                    .collect::<String>()
            ),
            "table" => format!("<section class=\"slide\">{title}{lede}{}{foot}</section>", table_html(&s.table, true, "", &[])),
            "chart" => format!(
                "<section class=\"slide\">{title}{lede}<div class=\"chart\">{}</div>{foot}</section>",
                s.chart.as_ref().map(|c| chart_svg(c, t, 980, 400)).unwrap_or_default()
            ),
            _ => format!("<section class=\"slide\">{title}{lede}{}{foot}</section>", points_html(&s.bullets)),
        };
        body.push_str(&html);
    }
    format!("<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><style>{css}</style></head><body>{body}</body></html>")
}

// ── Charts (SVG) ─────────────────────────────────────────────────────────
//
// Quiet: hairline gridlines, no axis boxes, labels in the muted colour, the
// accent on the lead series and neutrals for comparisons; lines labelled at
// their ends instead of a legend where that reads better.

/// A round step for an axis covering `0..=max`, about five ticks.
fn nice_step(max: f64) -> f64 {
    if max <= 0.0 {
        return 1.0;
    }
    let raw = max / 5.0;
    let mag = 10f64.powf(raw.log10().floor());
    let n = raw / mag;
    mag * if n <= 1.0 { 1.0 } else if n <= 2.0 { 2.0 } else if n <= 2.5 { 2.5 } else if n <= 5.0 { 5.0 } else { 10.0 }
}

fn short_num(v: f64, unit: &str) -> String {
    let a = v.abs();
    let s = if a >= 1e9 {
        format!("{:.1}B", v / 1e9)
    } else if a >= 1e6 {
        format!("{:.1}M", v / 1e6)
    } else if a >= 1e4 {
        format!("{:.0}k", v / 1e3)
    } else if v.fract() == 0.0 {
        group_thousands(v)
    } else {
        format!("{v:.1}")
    };
    let s = s.replace(".0B", "B").replace(".0M", "M");
    match unit {
        "" => s,
        "%" => format!("{s}%"),
        u if u.len() <= 2 && !u.chars().any(|c| c.is_alphabetic()) => format!("{u}{s}"),
        u => format!("{s} {u}"),
    }
}

pub fn chart_svg(c: &Chart, theme: &Theme, w: u32, h: u32) -> String {
    let pal = theme.palette();
    let (w, h) = (w as f64, h as f64);
    let muted = &theme.muted;
    let grid = &theme.line;
    let font = format!("font-family:{};font-size:11.5px;fill:#{muted};font-variant-numeric:tabular-nums", theme.body_css.replace('"', "'"));
    let mut s = format!("<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {w} {h}\" style=\"{font}\">");
    let ink = &theme.ink;

    if matches!(c.kind.as_str(), "pie" | "donut") {
        let values = &c.series[0].values;
        let total: f64 = values.iter().map(|v| v.max(0.0)).sum::<f64>().max(1e-9);
        let (cx, cy, r) = (h / 2.0, h / 2.0, h / 2.0 - 8.0);
        // Shades of the accent, darkest first, then neutrals.
        let shades: Vec<String> = (0..values.len())
            .map(|i| if i < 4 { mix(&theme.accent, &theme.paper, i as f32 * 0.22) } else { mix(&theme.ink, &theme.paper, 0.35 + (i - 4) as f32 * 0.15) })
            .collect();
        let mut a0 = -std::f64::consts::FRAC_PI_2;
        for (i, v) in values.iter().enumerate() {
            let frac = v.max(0.0) / total;
            let a1 = a0 + frac * std::f64::consts::TAU;
            let large = if frac > 0.5 { 1 } else { 0 };
            let (x0, y0, x1, y1) = (cx + r * a0.cos(), cy + r * a0.sin(), cx + r * a1.cos(), cy + r * a1.sin());
            if frac >= 0.9999 {
                s.push_str(&format!("<circle cx=\"{cx}\" cy=\"{cy}\" r=\"{r}\" fill=\"#{}\"/>", shades[i]));
            } else {
                s.push_str(&format!(
                    "<path d=\"M{cx},{cy} L{x0:.1},{y0:.1} A{r},{r} 0 {large} 1 {x1:.1},{y1:.1} Z\" fill=\"#{}\" stroke=\"#{}\" stroke-width=\"2\"/>",
                    shades[i], theme.paper
                ));
            }
            a0 = a1;
        }
        if c.kind == "donut" {
            s.push_str(&format!("<circle cx=\"{cx}\" cy=\"{cy}\" r=\"{:.1}\" fill=\"#{}\"/>", r * 0.62, theme.paper));
        }
        let lx = cx + r + 40.0;
        let rows = c.labels.len().max(1) as f64;
        let y0 = cy - (rows - 1.0) * 13.0;
        for (i, (label, v)) in c.labels.iter().zip(values).enumerate() {
            let y = y0 + i as f64 * 26.0;
            s.push_str(&format!(
                "<circle cx=\"{lx}\" cy=\"{:.1}\" r=\"4.5\" fill=\"#{}\"/><text x=\"{}\" y=\"{:.1}\" style=\"fill:#{ink}\">{}</text><text x=\"{}\" y=\"{:.1}\" text-anchor=\"end\" style=\"font-weight:600;fill:#{ink}\">{:.0}%</text>",
                y - 4.0,
                shades[i],
                lx + 14.0,
                y,
                esc(label),
                w - 8.0,
                y,
                v.max(0.0) / total * 100.0
            ));
        }
        s.push_str("</svg>");
        return s;
    }

    let max = c.series.iter().flat_map(|s| s.values.iter().copied()).fold(0.0f64, f64::max);
    let step = nice_step(max);
    let top = (max / step).ceil().max(1.0) * step;
    let multi = c.series.len() > 1;
    let line_like = matches!(c.kind.as_str(), "line" | "area");
    // Lines are labelled at their ends; bars get a small legend.
    let legend = multi && !line_like;
    let end_labels = multi && line_like;
    let label_w = if end_labels { c.series.iter().map(|s| s.name.chars().count()).max().unwrap_or(0) as f64 * 7.0 + 14.0 } else { 0.0 };
    let n = c.labels.len().max(1) as f64;

    if c.kind == "hbar" {
        let left = c.labels.iter().map(|l| l.chars().count()).max().unwrap_or(4) as f64 * 6.6 + 14.0;
        let (tp, right) = (if legend { 26.0 } else { 4.0 }, 60.0);
        let pw = w - left - right;
        let ph = h - tp - 4.0;
        if legend {
            legend_svg(&mut s, c, &pal, 0.0);
        }
        let band = ph / n;
        let k = c.series.len() as f64;
        let bar = (band * 0.62 / k).min(22.0);
        for (li, label) in c.labels.iter().enumerate() {
            let y0 = tp + band * li as f64 + (band - bar * k) / 2.0;
            s.push_str(&format!("<text x=\"0\" y=\"{:.1}\" style=\"fill:#{ink}\">{}</text>", y0 + bar * k / 2.0 + 4.0, esc(label)));
            for (si, se) in c.series.iter().enumerate() {
                let v = se.values.get(li).copied().unwrap_or(0.0).max(0.0);
                let bw = pw * v / top;
                s.push_str(&format!("<rect x=\"{left}\" y=\"{:.1}\" width=\"{bw:.1}\" height=\"{:.1}\" fill=\"#{}\"/>", y0 + bar * si as f64, bar - 2.0, pal[si % pal.len()]));
                if !multi {
                    s.push_str(&format!("<text x=\"{:.1}\" y=\"{:.1}\" style=\"fill:#{ink};font-weight:600\">{}</text>", left + bw + 6.0, y0 + bar / 2.0 + 4.0, short_num(v, &c.unit)));
                }
            }
        }
        s.push_str(&format!("<line x1=\"{left}\" y1=\"{tp}\" x2=\"{left}\" y2=\"{}\" stroke=\"#{}\"/>", tp + ph, theme.faint));
        s.push_str("</svg>");
        return s;
    }

    let left = (short_num(top, &c.unit).chars().count() as f64 * 7.0 + 12.0).max(34.0);
    let (tp, bottom, right) = (if legend { 30.0 } else { 10.0 }, 26.0, 8.0 + label_w);
    let (pw, ph) = (w - left - right, h - tp - bottom);
    if legend {
        legend_svg(&mut s, c, &pal, left);
    }
    // Gridlines and value labels.
    let ticks = (top / step).round() as i32;
    for k in 0..=ticks {
        let v = k as f64 * step;
        let y = tp + ph - ph * v / top;
        if k > 0 {
            s.push_str(&format!("<line x1=\"{left}\" y1=\"{y:.1}\" x2=\"{}\" y2=\"{y:.1}\" stroke=\"#{grid}\" stroke-width=\"1\"/>", left + pw));
        }
        s.push_str(&format!("<text x=\"{}\" y=\"{:.1}\" text-anchor=\"end\">{}</text>", left - 10.0, y + 4.0, short_num(v, &c.unit)));
    }
    let band = pw / n;
    let every = ((n * 54.0 / pw).ceil() as usize).max(1);
    for (li, label) in c.labels.iter().enumerate() {
        if li % every == 0 {
            let x = left + band * (li as f64 + 0.5);
            s.push_str(&format!("<text x=\"{x:.1}\" y=\"{}\" text-anchor=\"middle\">{}</text>", tp + ph + 19.0, esc(label)));
        }
    }
    if c.kind == "bar" {
        let k = c.series.len() as f64;
        let bar = (band * 0.62 / k).min(56.0);
        for li in 0..c.labels.len() {
            let x0 = left + band * li as f64 + (band - bar * k) / 2.0;
            for (si, se) in c.series.iter().enumerate() {
                let v = se.values.get(li).copied().unwrap_or(0.0).max(0.0);
                let bh = ph * v / top;
                s.push_str(&format!(
                    "<rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{bh:.1}\" fill=\"#{}\"/>",
                    x0 + bar * si as f64 + 1.0,
                    tp + ph - bh,
                    (bar - 2.0).max(2.0),
                    pal[si % pal.len()]
                ));
                if !multi && n <= 14.0 {
                    s.push_str(&format!(
                        "<text x=\"{:.1}\" y=\"{:.1}\" text-anchor=\"middle\" style=\"fill:#{ink};font-weight:600\">{}</text>",
                        x0 + bar / 2.0,
                        tp + ph - bh - 7.0,
                        short_num(v, &c.unit)
                    ));
                }
            }
        }
    } else {
        // Draw comparisons first so the lead series sits on top.
        for (si, se) in c.series.iter().enumerate().rev() {
            let col = &pal[si % pal.len()];
            let pts: Vec<(f64, f64)> = se.values.iter().enumerate().map(|(i, v)| (left + band * (i as f64 + 0.5), tp + ph - ph * v.max(0.0) / top)).collect();
            let path: String = pts.iter().enumerate().map(|(i, (x, y))| format!("{}{x:.1},{y:.1}", if i == 0 { "M" } else { " L" })).collect();
            if c.kind == "area" && si == 0 {
                if let (Some(first), Some(last)) = (pts.first(), pts.last()) {
                    s.push_str(&format!("<path d=\"{path} L{:.1},{} L{:.1},{} Z\" fill=\"#{col}\" fill-opacity=\"0.12\"/>", last.0, tp + ph, first.0, tp + ph));
                }
            }
            let width = if si == 0 { 2.4 } else { 1.8 };
            s.push_str(&format!("<path d=\"{path}\" fill=\"none\" stroke=\"#{col}\" stroke-width=\"{width}\" stroke-linejoin=\"round\" stroke-linecap=\"round\"/>"));
            if let Some((x, y)) = pts.last() {
                s.push_str(&format!("<circle cx=\"{x:.1}\" cy=\"{y:.1}\" r=\"3.2\" fill=\"#{col}\"/>"));
                if end_labels {
                    s.push_str(&format!("<text x=\"{:.1}\" y=\"{:.1}\" style=\"fill:#{col};font-weight:600\">{}</text>", x + 9.0, y + 4.0, esc(&se.name)));
                } else if !multi {
                    s.push_str(&format!("<text x=\"{:.1}\" y=\"{:.1}\" text-anchor=\"middle\" style=\"fill:#{ink};font-weight:600\">{}</text>", x, y - 10.0, short_num(se.values.last().copied().unwrap_or(0.0), &c.unit)));
                }
            }
        }
    }
    s.push_str(&format!("<line x1=\"{left}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"#{}\" stroke-width=\"1\"/>", tp + ph, left + pw, tp + ph, theme.faint));
    s.push_str("</svg>");
    s
}

fn legend_svg(s: &mut String, c: &Chart, pal: &[String], x0: f64) {
    let mut x = x0;
    for (i, se) in c.series.iter().enumerate() {
        s.push_str(&format!("<rect x=\"{x}\" y=\"5\" width=\"9\" height=\"9\" fill=\"#{}\"/><text x=\"{}\" y=\"13.5\">{}</text>", pal[i % pal.len()], x + 14.0, esc(&se.name)));
        x += 30.0 + se.name.chars().count() as f64 * 6.8;
    }
}

// ── The system's browser ─────────────────────────────────────────────────

/// Edge, Chrome or another Chromium installed on this machine.
pub fn find_browser() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("VIV_CHROMIUM") {
        let p = PathBuf::from(p);
        if p.exists() {
            return Some(p);
        }
    }
    let mut candidates: Vec<PathBuf> = Vec::new();
    #[cfg(windows)]
    {
        for base in ["ProgramFiles(x86)", "ProgramFiles", "LOCALAPPDATA"].iter().filter_map(|v| std::env::var(v).ok()) {
            for rel in [
                r"Microsoft\Edge\Application\msedge.exe",
                r"Google\Chrome\Application\chrome.exe",
                r"BraveSoftware\Brave-Browser\Application\brave.exe",
                r"Chromium\Application\chrome.exe",
            ] {
                candidates.push(Path::new(&base).join(rel));
            }
        }
    }
    #[cfg(target_os = "macos")]
    {
        for app in [
            "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
            "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
            "/Applications/Brave Browser.app/Contents/MacOS/Brave Browser",
            "/Applications/Chromium.app/Contents/MacOS/Chromium",
        ] {
            candidates.push(PathBuf::from(app));
        }
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        for name in ["google-chrome", "google-chrome-stable", "chromium", "chromium-browser", "microsoft-edge", "brave-browser"] {
            if let Some(path) = std::env::var_os("PATH") {
                for dir in std::env::split_paths(&path) {
                    candidates.push(dir.join(name));
                }
            }
        }
    }
    candidates.into_iter().find(|p| p.is_file())
}

/// Run the browser headless on an HTML string. `what` is the output flag
/// (`--print-to-pdf=…` or `--screenshot=…`), plus any extra flags.
fn run_headless(html: &str, flags: &[String], out: &Path) -> Result<(), String> {
    let browser = find_browser().ok_or("no Chromium browser (Edge, Chrome, Brave) is installed")?;
    let work = std::env::temp_dir().join(format!("viv-doc-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&work).map_err(|e| e.to_string())?;
    let page = work.join("doc.html");
    std::fs::write(&page, html).map_err(|e| e.to_string())?;
    let url = url::Url::from_file_path(&page).map_err(|_| "bad temp path".to_string())?;
    let _ = std::fs::remove_file(out);

    let mut cmd = std::process::Command::new(&browser);
    cmd.args([
        "--headless=new",
        "--disable-gpu",
        "--no-first-run",
        "--no-default-browser-check",
        "--disable-extensions",
        "--hide-scrollbars",
        "--mute-audio",
        "--no-pdf-header-footer",
        "--run-all-compositor-stages-before-draw",
        "--virtual-time-budget=4000",
    ])
    .arg(format!("--user-data-dir={}", work.join("profile").display()))
    .args(flags)
    .arg(url.as_str())
    .stdout(std::process::Stdio::null())
    .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let mut child = cmd.spawn().map_err(|e| format!("couldn't start {}: {e}", browser.display()))?;
    let deadline = std::time::Instant::now() + Duration::from_secs(90);
    let result = loop {
        match child.try_wait() {
            Ok(Some(_)) => break Ok(()),
            Ok(None) if std::time::Instant::now() > deadline => {
                let _ = child.kill();
                break Err("the browser took too long to render the document".to_string());
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(100)),
            Err(e) => break Err(e.to_string()),
        }
    };
    // The browser may still hold its profile for a moment after exiting.
    for _ in 0..10 {
        if std::fs::remove_dir_all(&work).is_ok() {
            break;
        }
        std::thread::sleep(Duration::from_millis(150));
    }
    result?;
    match std::fs::metadata(out) {
        Ok(m) if m.len() > 0 => Ok(()),
        _ => Err("the browser didn't produce any output".into()),
    }
}

pub fn print_pdf(html: &str, out: &Path) -> Result<(), String> {
    run_headless(html, &[format!("--print-to-pdf={}", out.display())], out)
}

/// A PNG of the page's first `w`×`h` CSS pixels, at `scale`× resolution.
pub fn screenshot(html: &str, w: u32, h: u32, scale: f32, transparent: bool) -> Result<Vec<u8>, String> {
    let out = std::env::temp_dir().join(format!("viv-shot-{}.png", uuid::Uuid::new_v4()));
    let mut flags = vec![format!("--screenshot={}", out.display()), format!("--window-size={w},{h}"), format!("--force-device-scale-factor={scale}")];
    if transparent {
        flags.push("--default-background-color=00000000".into());
    }
    run_headless(html, &flags, &out)?;
    let bytes = std::fs::read(&out).map_err(|e| e.to_string());
    let _ = std::fs::remove_file(&out);
    bytes
}

/// A chart as a PNG (2× for print), for Word and PowerPoint.
pub fn chart_png(c: &Chart, theme: &Theme, w: u32, h: u32) -> Result<Vec<u8>, String> {
    let svg = chart_svg(c, theme, w, h);
    let bg = if theme.dark { "#111827" } else { "#ffffff" };
    let html = format!("<!doctype html><html><head><meta charset=\"utf-8\"><style>html,body{{margin:0;background:{bg}}}svg{{display:block;width:{w}px;height:{h}px}}</style></head><body>{svg}</body></html>");
    screenshot(&html, w, h, 2.0, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inline_markup() {
        let s = spans("Plain **bold** and *it* with `x_y` and [a link](https://e.x) and snake_case_name");
        assert!(s.iter().any(|x| x.text == "bold" && x.bold));
        assert!(s.iter().any(|x| x.text == "it" && x.italic));
        assert!(s.iter().any(|x| x.text == "x_y" && x.code));
        assert!(s.iter().any(|x| x.text == "a link" && x.link.as_deref() == Some("https://e.x")));
        assert_eq!(plain("snake_case_name stays"), "snake_case_name stays");
        assert_eq!(plain("2 * 3 = 6"), "2 * 3 = 6");
    }

    #[test]
    fn themes_and_colours() {
        assert_eq!(hex("#abc").as_deref(), Some("AABBCC"));
        assert_eq!(hex("4f46e5").as_deref(), Some("4F46E5"));
        assert_eq!(hex("nope"), None);
        assert_eq!(mix("000000", "FFFFFF", 0.5), "808080");
        let t = Theme::from_arg(Some(&serde_json::json!({ "preset": "classic", "accent": "#123456" })));
        assert_eq!(t.accent, "123456");
        assert!(t.heading_css.contains("Source Serif") && t.serif_headings());
    }

    #[test]
    fn numbers_read_well() {
        assert_eq!(cell_text(&serde_json::json!(1234567)), "1,234,567");
        assert_eq!(cell_text(&serde_json::json!(-1234.5)), "-1,234.5");
        assert!(looks_numeric("$1,200") && looks_numeric("12%") && !looks_numeric("Q1"));
        assert_eq!(nice_step(97.0), 20.0);
    }

    #[test]
    fn lists_nest_and_html_balances() {
        let blocks = parse_blocks(
            Some(&serde_json::json!([{ "type": "bullets", "items": ["a", { "text": "b", "items": ["b1", "b2"] }, "c"] }])),
            Path::new("."),
        )
        .unwrap();
        let html = blocks_html(&blocks, &Theme::preset("modern"));
        assert_eq!(html.matches("<ul>").count(), html.matches("</ul>").count());
        assert_eq!(html.matches("<li>").count(), html.matches("</li>").count());
        assert!(html.contains("b1"));
    }

    #[test]
    fn charts_render_every_kind() {
        for kind in ["bar", "hbar", "line", "area", "pie", "donut"] {
            let c = parse_chart(&serde_json::json!({ "kind": kind, "labels": ["Q1", "Q2", "Q3"], "series": [{ "name": "Revenue", "values": [10, 25.5, 18] }] })).unwrap();
            let svg = chart_svg(&c, &Theme::preset("modern"), 600, 300);
            assert!(svg.starts_with("<svg") && svg.ends_with("</svg>"), "{kind}");
            assert!(!svg.contains("NaN"), "{kind}");
        }
        assert!(parse_chart(&serde_json::json!({ "kind": "radar", "labels": ["a"], "values": [1] })).is_err());
    }

    #[test]
    fn slides_pick_a_layout() {
        let s = parse_slides(
            &serde_json::json!([
                { "title": "Deck", "subtitle": "Q3" },
                { "title": "Points", "bullets": ["a"] },
                { "title": "By the numbers", "stats": [{ "value": "42%", "label": "growth" }] },
                { "quote": "Ship it", "by": "Someone" }
            ]),
            Path::new("."),
        )
        .unwrap();
        let layouts: Vec<&str> = s.iter().map(|x| x.layout.as_str()).collect();
        assert_eq!(layouts, ["title", "bullets", "stats", "quote"]);
        let html = deck_html(&s, &Theme::preset("modern"));
        assert_eq!(html.matches("class=\"slide").count(), 4);
    }
}

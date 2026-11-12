//! Commands behind the document viewer: read a file for preview, change one
//! part of it, and put earlier bytes back for undo.
//!
//! Editing goes through `agent::tools::files::documents::apply_edit` — the same
//! in-place XML edit the agent's `edit_document` tool uses — so a change made
//! by hand in the viewer keeps every style, formula and image exactly as an
//! agent edit does. The viewer never regenerates a file.

use base64::Engine as _;
use std::path::Path;

/// Formats the viewer can show. Legacy binary Office formats (.doc, .ppt)
/// aren't here: nothing in the app can read them, and pretending otherwise
/// would show an empty page. Spreadsheets in older formats (.xls, .ods) and
/// CSV are previewable — the spreadsheet reader handles them.
const VIEWABLE: &[&str] = &["pdf", "docx", "xlsx", "xlsm", "xls", "ods", "csv", "pptx"];
/// Formats `apply_edit` can change in place.
const EDITABLE: &[&str] = &["docx", "xlsx", "pptx"];
const MAX_BYTES: u64 = 64 * 1024 * 1024;

fn ext(path: &Path) -> String {
    path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase()
}

fn check(path: &Path, allowed: &[&str]) -> Result<(), String> {
    let e = ext(path);
    if !allowed.contains(&e.as_str()) {
        return Err(format!("'.{e}' files aren't supported here (supported: {})", allowed.join(", ")));
    }
    let meta = std::fs::metadata(path).map_err(|e| format!("Can't open {}: {e}", path.display()))?;
    if !meta.is_file() {
        return Err(format!("{} is not a file", path.display()));
    }
    if meta.len() > MAX_BYTES {
        return Err(format!("{} is larger than {} MB", path.display(), MAX_BYTES / 1024 / 1024));
    }
    Ok(())
}

/// The file's bytes, base64-encoded, for the viewer to render.
#[tauri::command]
pub async fn cmd_read_document_bytes(path: String) -> Result<String, String> {
    tokio::task::spawn_blocking(move || {
        let p = Path::new(&path);
        check(p, VIEWABLE)?;
        let bytes = std::fs::read(p).map_err(|e| format!("Can't read {path}: {e}"))?;
        Ok(base64::engine::general_purpose::STANDARD.encode(bytes))
    })
    .await
    .map_err(|e| format!("read task failed: {e}"))?
}

/// Characters of a document's text inlined into the prompt; the rest stays
/// readable through `read` on the saved copy.
const ATTACH_MAX_CHARS: usize = 120_000;
/// Pages without a text layer rendered for an attached PDF.
const ATTACH_MAX_PAGES: usize = 6;

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachedImage {
    pub base64: String,
    pub media_type: String,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentReading {
    /// What goes into the prompt in place of the file's bytes.
    pub content: String,
    pub truncated: bool,
    /// Where the copy was saved, so the agent can `read` more of it.
    pub path: String,
    /// Page images for pages with no readable text (scans, unmapped fonts).
    pub images: Vec<AttachedImage>,
}

/// A document attached in chat (PDF, Word, PowerPoint, spreadsheet): parse
/// it here, where real parsers live, instead of letting the webview read the
/// zip or PDF bytes as text. The file is saved to a temp folder so the agent
/// can come back to pages that weren't inlined.
#[tauri::command]
pub async fn cmd_extract_attachment(name: String, base64: String) -> Result<AttachmentReading, String> {
    use crate::agent::tools::files::extract;
    tokio::task::spawn_blocking(move || {
        let bytes = base64::engine::general_purpose::STANDARD.decode(base64.as_bytes()).map_err(|e| format!("bad attachment data: {e}"))?;
        if bytes.len() as u64 > MAX_BYTES {
            return Err(format!("{name} is larger than {} MB", MAX_BYTES / 1024 / 1024));
        }
        // Only the file name, never a path the webview made up.
        let safe: String = Path::new(&name)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("attachment")
            .chars()
            .map(|c| if c.is_control() || "<>:\"/\\|?*".contains(c) { '_' } else { c })
            .collect();
        let dir = std::env::temp_dir().join("vivianne-attachments").join(uuid::Uuid::new_v4().to_string());
        std::fs::create_dir_all(&dir).map_err(|e| format!("couldn't save the attachment: {e}"))?;
        let path = dir.join(&safe);
        std::fs::write(&path, &bytes).map_err(|e| format!("couldn't save the attachment: {e}"))?;
        let shown = path.to_string_lossy().to_string();

        let reading = extract::read(&path, &extract::Want::default())?;
        let what = match reading.kind {
            "pdf" => format!("PDF, {} page{}", reading.total, if reading.total == 1 { "" } else { "s" }),
            "pptx" => format!("PowerPoint, {} slide{}", reading.total, if reading.total == 1 { "" } else { "s" }),
            "docx" => "Word document".to_string(),
            _ => "spreadsheet".to_string(),
        };
        let mut images = Vec::new();
        let mut rendered = Vec::new();
        if reading.kind == "pdf" && !reading.visual_pages.is_empty() {
            let take: Vec<usize> = reading.visual_pages.iter().copied().take(ATTACH_MAX_PAGES).collect();
            for (n, png) in extract::pdf_render(&bytes, &take).unwrap_or_default() {
                rendered.push(n);
                images.push(AttachedImage { base64: base64::engine::general_purpose::STANDARD.encode(png), media_type: "image/png".into() });
            }
        }
        let truncated = reading.text.chars().count() > ATTACH_MAX_CHARS;
        let text: String = if truncated { reading.text.chars().take(ATTACH_MAX_CHARS).collect() } else { reading.text.clone() };
        let mut content = format!("[{safe}: {what}, parsed from the original file]\nSaved copy: {shown}\n\n{text}");
        if truncated {
            content.push_str("\n\n[Only the start is shown. Read the rest from the saved copy with the read tool and its pages (or sheet) argument.]");
        }
        if !rendered.is_empty() {
            let list = rendered.iter().map(|n| n.to_string()).collect::<Vec<_>>().join(", ");
            content.push_str(&format!("\n\n[Pages {list} have no text layer; their images are attached to this message, in that order. Read them from the images.]"));
        }
        let left = reading.visual_pages.len() - rendered.len();
        if left > 0 {
            content.push_str(&format!("\n[{left} more page(s) without a text layer: read them from the saved copy with read, pages and render: true.]"));
        }
        if reading.kind == "pdf" {
            content.push_str("\n[If a figure, an equation or the layout matters, read that page from the saved copy with read, pages and render: true.]");
        }
        Ok(AttachmentReading { content, truncated, path: shown, images })
    })
    .await
    .map_err(|e| format!("attachment task failed: {e}"))?
}

/// Replace one part — a paragraph (`p:3`), a cell (`Sheet1!B4`) or a text
/// box (`slide:2:1`) — leaving the rest of the file byte-identical.
#[tauri::command]
pub async fn cmd_edit_document_part(path: String, part: String, text: String) -> Result<String, String> {
    tokio::task::spawn_blocking(move || {
        let p = Path::new(&path);
        check(p, EDITABLE)?;
        crate::agent::tools::files::documents::apply_edit(p, &part, &text)
    })
    .await
    .map_err(|e| format!("edit task failed: {e}"))?
}

/// Put earlier bytes back — the viewer's undo. Only over an existing document
/// of a viewable type, so this can't be used to create or clobber other files.
#[tauri::command]
pub async fn cmd_restore_document_bytes(path: String, base64: String) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        let p = Path::new(&path);
        check(p, VIEWABLE)?;
        let bytes = base64::engine::general_purpose::STANDARD.decode(base64.as_bytes()).map_err(|e| format!("Bad data: {e}"))?;
        let tmp = p.with_extension(format!("{}.viv-restore", ext(p)));
        std::fs::write(&tmp, &bytes).map_err(|e| format!("Can't write {}: {e}", tmp.display()))?;
        std::fs::rename(&tmp, p).map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            format!("Can't restore {path}: {e}")
        })
    })
    .await
    .map_err(|e| format!("restore task failed: {e}"))?
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The viewer's real round trip: edit a cell in place, see it changed,
    /// then undo by restoring the bytes read before the edit.
    #[tokio::test]
    async fn edit_then_restore_a_real_spreadsheet() {
        let dir = std::env::temp_dir().join(format!("viv-office-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("book.xlsx");
        let mut wb = rust_xlsxwriter::Workbook::new();
        let ws = wb.add_worksheet().set_name("Sales").unwrap();
        ws.write_string(0, 0, "Units").unwrap();
        ws.write_number(1, 0, 120.0).unwrap();
        wb.save(&path).unwrap();
        let p = path.to_string_lossy().to_string();

        let original = cmd_read_document_bytes(p.clone()).await.unwrap();
        cmd_edit_document_part(p.clone(), "Sales!A2".into(), "125".into()).await.unwrap();
        let cell = |parts: Vec<crate::agent::tools::files::documents::DocPart>| {
            parts.into_iter().find(|x| x.id == "Sales!A2").map(|x| x.text).unwrap_or_default()
        };
        assert_eq!(cell(crate::agent::tools::files::documents::inspect(&path).unwrap()), "125");

        cmd_restore_document_bytes(p.clone(), original.clone()).await.unwrap();
        assert_eq!(cell(crate::agent::tools::files::documents::inspect(&path).unwrap()), "120");
        assert_eq!(cmd_read_document_bytes(p).await.unwrap(), original);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn only_documents_pass_the_check() {
        let dir = std::env::temp_dir().join(format!("viv-office-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let doc = dir.join("a.docx");
        std::fs::write(&doc, b"x").unwrap();
        let exe = dir.join("a.exe");
        std::fs::write(&exe, b"x").unwrap();
        assert!(check(&doc, VIEWABLE).is_ok());
        assert!(check(&exe, VIEWABLE).is_err());
        assert!(check(&dir.join("missing.docx"), VIEWABLE).is_err());
        assert!(check(&dir.join("a.pdf"), EDITABLE).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}

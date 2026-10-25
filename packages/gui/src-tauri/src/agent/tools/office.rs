//! Commands behind the document viewer: read a file for preview, change one
//! part of it, and put earlier bytes back for undo.
//!
//! Editing goes through `coding_agent::documents::apply_edit` — the same
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

/// Replace one part — a paragraph (`p:3`), a cell (`Sheet1!B4`) or a text
/// box (`slide:2:1`) — leaving the rest of the file byte-identical.
#[tauri::command]
pub async fn cmd_edit_document_part(path: String, part: String, text: String) -> Result<String, String> {
    tokio::task::spawn_blocking(move || {
        let p = Path::new(&path);
        check(p, EDITABLE)?;
        crate::coding_agent::documents::apply_edit(p, &part, &text)
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
        let cell = |parts: Vec<crate::coding_agent::documents::DocPart>| {
            parts.into_iter().find(|x| x.id == "Sales!A2").map(|x| x.text).unwrap_or_default()
        };
        assert_eq!(cell(crate::coding_agent::documents::inspect(&path).unwrap()), "125");

        cmd_restore_document_bytes(p.clone(), original.clone()).await.unwrap();
        assert_eq!(cell(crate::coding_agent::documents::inspect(&path).unwrap()), "120");
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

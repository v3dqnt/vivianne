//! Round-trip check: generate a real document, inspect its parts, edit one,
//! and confirm the change landed and the file is still a valid archive.
//!
//! The unit tests cover the XML rewriting on synthetic markup. This runs the
//! whole path against files that `create_document` actually produced, which is
//! where the real risks live — a rewritten zip that Word refuses to open would
//! pass every unit test in the module.

use app_lib::coding_agent::document_edit::{apply_edit, inspect};
use app_lib::coding_agent::documents::CreateDocumentTool;
use app_lib::coding_agent::tool_exec::Tool;
use serde_json::json;

#[tokio::main]
async fn main() {
    let tmp = std::env::temp_dir().join(format!("viv-doc-rt-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).expect("tmp");
    let cwd = tmp.to_string_lossy().to_string();
    let tool = CreateDocumentTool::new(cwd.clone());

    let mut failures = 0;
    let mut check = |name: &str, ok: bool, detail: String| {
        println!("{} {name}: {detail}", if ok { "PASS" } else { "FAIL" });
        if !ok {
            failures += 1;
        }
    };

    // ── DOCX ────────────────────────────────────────────────────────────────
    let r = tool
        .execute(
            &json!({
                "path": "report.docx", "format": "docx", "title": "Quarterly",
                "blocks": [
                    { "type": "heading", "text": "Summary" },
                    { "type": "paragraph", "text": "Revenue was flat this quarter." },
                    { "type": "paragraph", "text": "Costs rose slightly." }
                ]
            }),
            &cwd,
        )
        .await;
    check("docx generated", !r.is_error, r.content.clone());

    let docx = tmp.join("report.docx");
    let parts = inspect(&docx).expect("inspect docx");
    check("docx parts found", parts.len() >= 3, format!("{} parts", parts.len()));

    // Target the paragraph by its text, not a guessed index — the generator is
    // free to emit extra paragraphs (spacers, the title) and this check should
    // fail on a genuine bug rather than on layout.
    let target = parts
        .iter()
        .find(|p| p.text.contains("Revenue was flat"))
        .expect("the seeded paragraph should be listed");
    apply_edit(&docx, &target.id, "Revenue GREW 12% this quarter.").expect("edit docx");

    let after = inspect(&docx).expect("re-inspect docx");
    check(
        "docx edit applied",
        after.iter().any(|p| p.text.contains("GREW 12%")),
        after.iter().find(|p| p.id == target.id).map(|p| p.text.clone()).unwrap_or_default(),
    );
    check(
        "docx untouched paragraph survived",
        after.iter().any(|p| p.text.contains("Costs rose slightly")),
        "neighbouring paragraph intact".into(),
    );
    check(
        "docx still a valid archive",
        zip::ZipArchive::new(std::fs::File::open(&docx).unwrap()).is_ok(),
        "zip opens".into(),
    );

    // ── XLSX ────────────────────────────────────────────────────────────────
    let r = tool
        .execute(
            &json!({
                "path": "sheet.xlsx", "format": "xlsx",
                "sheets": [ { "name": "Data", "rows": [["Item", "Cost"], ["Widget", 10], ["Gadget", 20]] } ]
            }),
            &cwd,
        )
        .await;
    check("xlsx generated", !r.is_error, r.content.clone());

    let xlsx = tmp.join("sheet.xlsx");
    let parts = inspect(&xlsx).expect("inspect xlsx");
    let cell = parts.iter().find(|p| p.text == "Widget").expect("Widget cell listed");
    apply_edit(&xlsx, &cell.id, "Sprocket").expect("edit xlsx");
    let after = inspect(&xlsx).expect("re-inspect xlsx");
    check(
        "xlsx cell edited",
        after.iter().any(|p| p.text == "Sprocket"),
        format!("{} -> Sprocket", cell.id),
    );
    check(
        "xlsx other cells survived",
        after.iter().any(|p| p.text == "Gadget"),
        "neighbouring cell intact".into(),
    );

    // A numeric edit must stay numeric, or the user's SUM() silently breaks.
    if let Some(num) = after.iter().find(|p| p.text == "20").map(|p| p.id.clone()) {
        apply_edit(&xlsx, &num, "35").expect("edit number");
        let sheet_xml = {
            let f = std::fs::File::open(&xlsx).unwrap();
            let mut z = zip::ZipArchive::new(f).unwrap();
            let mut s = String::new();
            use std::io::Read;
            z.by_name("xl/worksheets/sheet1.xml").unwrap().read_to_string(&mut s).unwrap();
            s
        };
        check(
            "numeric cell written as a number, not text",
            sheet_xml.contains("<v>35</v>"),
            "stored as <v>35</v>".into(),
        );
    }

    // ── PPTX ────────────────────────────────────────────────────────────────
    let r = tool
        .execute(
            &json!({
                "path": "deck.pptx", "format": "pptx",
                "slides": [ { "title": "Agenda", "bullets": ["First point", "Second point"] } ]
            }),
            &cwd,
        )
        .await;
    check("pptx generated", !r.is_error, r.content.clone());

    let pptx = tmp.join("deck.pptx");
    let parts = inspect(&pptx).expect("inspect pptx");
    check("pptx parts found", !parts.is_empty(), format!("{} parts", parts.len()));
    if let Some(t) = parts.iter().find(|p| p.text.contains("Agenda")) {
        apply_edit(&pptx, &t.id, "Revised Agenda").expect("edit pptx");
        let after = inspect(&pptx).expect("re-inspect pptx");
        check(
            "pptx text edited",
            after.iter().any(|p| p.text.contains("Revised Agenda")),
            "slide text replaced".into(),
        );
    }

    // ── PDF refuses text edits rather than corrupting ───────────────────────
    let r = tool
        .execute(
            &json!({
                "path": "doc.pdf", "format": "pdf", "title": "Notes",
                "blocks": [{ "type": "paragraph", "text": "Hello" }]
            }),
            &cwd,
        )
        .await;
    check("pdf generated", !r.is_error, r.content.clone());
    let pdf = tmp.join("doc.pdf");
    let refused = apply_edit(&pdf, "page:1", "new text");
    check(
        "pdf text edit refused with a reason",
        refused.is_err(),
        refused.err().unwrap_or_default().chars().take(80).collect::<String>(),
    );

    let _ = std::fs::remove_dir_all(&tmp);
    println!("\n{}", if failures == 0 { "ALL CHECKS PASSED" } else { "SOME CHECKS FAILED" });
    std::process::exit(if failures == 0 { 0 } else { 1 });
}

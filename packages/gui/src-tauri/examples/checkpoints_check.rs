//! End-to-end check for the checkpoint layer, against real files on disk.
//!
//! Covers the two cases that matter and are easy to get wrong: restoring an
//! edit to a file that already existed, and undoing the *creation* of a file
//! that did not (which must delete it, not leave an empty one).

use app_lib::agent::checkpoints as cp;

fn main() {
    let tmp = std::env::temp_dir().join(format!("viv-cp-check-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).expect("tmp dir");
    let session = format!("checkcheck-{}", std::process::id());

    let existing = tmp.join("existing.txt");
    let created = tmp.join("created.txt");
    std::fs::write(&existing, "ORIGINAL").expect("seed");

    let mut failures = 0;
    let mut check = |name: &str, ok: bool, detail: String| {
        println!("{} {name}: {detail}", if ok { "PASS" } else { "FAIL" });
        if !ok {
            failures += 1;
        }
    };

    let paths = vec![
        existing.to_string_lossy().into_owned(),
        created.to_string_lossy().into_owned(),
    ];
    let checkpoint = cp::capture(&session, "tc-1", "write", &paths).expect("capture");
    check("capture recorded both files", checkpoint.files.len() == 2, format!("{} files", checkpoint.files.len()));
    let absent_recorded = checkpoint.files.iter().any(|f| f.body.is_none());
    check("absent file recorded as absent", absent_recorded, "created.txt had no prior body".into());

    // Simulate the agent's edits.
    std::fs::write(&existing, "AGENT OVERWROTE THIS").expect("edit");
    std::fs::write(&created, "AGENT CREATED THIS").expect("create");

    let report = cp::restore(&session, &checkpoint.id).expect("restore");
    check(
        "existing file restored to original",
        std::fs::read_to_string(&existing).unwrap_or_default() == "ORIGINAL",
        std::fs::read_to_string(&existing).unwrap_or_default(),
    );
    check(
        "created file removed, not emptied",
        !created.exists(),
        format!("exists={}", created.exists()),
    );
    check("nothing failed", report.failed.is_empty(), format!("{:?}", report.failed));

    // Restoring is itself destructive, so it must have left an undo behind.
    let all = cp::list(&session);
    check(
        "restore captured an undo point",
        all.iter().any(|c| c.tool_name == "restore"),
        format!("{} checkpoints total", all.len()),
    );

    // Newest-first ordering is what the UI relies on.
    let sorted = all.windows(2).all(|w| w[0].created_at_ms >= w[1].created_at_ms);
    check("listed newest first", sorted, "ordering".into());

    cp::clear(&session);
    check("clear removes everything", cp::list(&session).is_empty(), "cleared".into());

    let _ = std::fs::remove_dir_all(&tmp);
    println!("\n{}", if failures == 0 { "ALL CHECKS PASSED" } else { "SOME CHECKS FAILED" });
    std::process::exit(if failures == 0 { 0 } else { 1 });
}

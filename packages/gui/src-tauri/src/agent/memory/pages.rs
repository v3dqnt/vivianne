//! Project pages: a few named questions per project ("How do we release?", "What are our conventions?") whose answers are kept up to date.
//!
//! The idea is Hindsight's mental models and pages (vectorize-io/hindsight, MIT): a standing answer that refreshes itself as the evidence
//! changes and is edited, not regenerated, so detail is not eroded by rewriting. Here a page is a list of bullets in an ordinary memory row
//! (`kind = page`), so recall finds it by keyword and by meaning like any other memory.
//!
//! - **Defined by the user**, per project, in the memory settings. Nothing is created on its own: each page costs model calls.
//! - **Refreshed only when its evidence changed.** The facts, observations and notes nearest the question are fingerprinted; an unchanged
//!   fingerprint costs nothing. At most `MAX_REFRESH_PER_PASS` pages are refreshed per sleep pass.
//! - **Edited by deltas** (add, edit, remove a bullet), with the same machinery as the user profile (`evolve`).
//! - **Titles only in the always-on block.** The page names are listed there; their bodies are found through `recall`.

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use super::MemoryEntry;

pub const MAX_PAGES: usize = 12;
pub const MAX_REFRESH_PER_PASS: usize = 3;
const MAX_NAME: usize = 60;
const MAX_QUESTION: usize = 300;
/// Pieces of evidence shown to the model per refresh.
const EVIDENCE: usize = 12;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PageDef {
    pub name: String,
    pub question: String,
}

/// The page definitions to keep: trimmed, with no empty or repeated names, none too long, and at most `MAX_PAGES`.
pub fn clean_defs(defs: Vec<PageDef>) -> Result<Vec<PageDef>, String> {
    let mut out: Vec<PageDef> = Vec::new();
    for d in defs {
        let (name, question) = (d.name.trim().to_string(), d.question.trim().to_string());
        if name.is_empty() || question.is_empty() {
            return Err("a page needs a name and a question".to_string());
        }
        if name.chars().count() > MAX_NAME || question.chars().count() > MAX_QUESTION {
            return Err(format!("a page name is at most {MAX_NAME} characters and its question at most {MAX_QUESTION}"));
        }
        if out.iter().any(|p| p.name.to_lowercase() == name.to_lowercase()) {
            return Err(format!("there are two pages called '{name}'"));
        }
        out.push(PageDef { name, question });
    }
    if out.len() > MAX_PAGES {
        return Err(format!("a project keeps at most {MAX_PAGES} pages"));
    }
    Ok(out)
}

pub fn init(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS memory_pages (
            name TEXT PRIMARY KEY,
            entry_id TEXT NOT NULL,
            evidence_key TEXT NOT NULL,
            refreshed_at INTEGER NOT NULL
        );",
    )
    .map_err(|e| format!("failed to init page schema: {e}"))
}

/// A fingerprint of the evidence a page was last built from: FNV-1a over each piece's id and text, in order.
pub fn evidence_key(evidence: &[MemoryEntry]) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for e in evidence {
        for b in e.id.bytes().chain(std::iter::once(0)).chain(e.text.bytes()).chain(std::iter::once(0)) {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
    }
    format!("{h:016x}")
}

pub fn row(conn: &Connection, name: &str) -> Option<(String, String)> {
    conn.query_row("SELECT entry_id, evidence_key FROM memory_pages WHERE name = ?1", params![name], |r| Ok((r.get(0)?, r.get(1)?))).optional().ok().flatten()
}

pub fn put_row(conn: &Connection, name: &str, entry_id: &str, key: &str, now: u64) -> Result<(), String> {
    conn.execute(
        "INSERT INTO memory_pages (name, entry_id, evidence_key, refreshed_at) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(name) DO UPDATE SET entry_id = ?2, evidence_key = ?3, refreshed_at = ?4",
        params![name, entry_id, key, now as i64],
    )
    .map(|_| ())
    .map_err(|e| e.to_string())
}

/// Page names that no longer have a definition, with the memory rows that held them.
pub fn orphans(conn: &Connection, keep: &[String]) -> Vec<(String, String)> {
    let Ok(mut stmt) = conn.prepare("SELECT name, entry_id FROM memory_pages") else { return Vec::new() };
    let Ok(rows) = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))) else { return Vec::new() };
    rows.flatten().filter(|(n, _)| !keep.contains(n)).collect()
}

pub fn forget_row(conn: &Connection, name: &str) -> Result<(), String> {
    conn.execute("DELETE FROM memory_pages WHERE name = ?1", params![name]).map(|_| ()).map_err(|e| e.to_string())
}

/// A page's stored text: its title, then one bullet per line.
pub fn render(name: &str, bullets: &[String]) -> String {
    format!("{name}\n{}", bullets.iter().map(|b| format!("- {b}")).collect::<Vec<_>>().join("\n"))
}

/// The bullets of a stored page (the title line is dropped).
pub fn bullets_of(stored: &str) -> Vec<String> {
    super::evolve::profile_bullets(stored.split_once('\n').map(|(_, rest)| rest).unwrap_or(""))
}

const PAGE_PROMPT: &str = "You keep one page of an agent's project memory: a short answer to a standing question, as numbered bullets. \
You are shown the QUESTION, the CURRENT bullets and EVIDENCE (numbered facts, observations and notes from the project). \
Return only the changes as a JSON array, no prose: [{\"op\": \"add\", \"text\": \"...\"}, {\"op\": \"edit\", \"n\": 3, \"text\": \"...\"}, {\"op\": \"remove\", \"n\": 5}]. \
Add what the evidence shows that answers the question and is missing, edit a bullet the evidence refines or corrects, remove one only when the evidence makes it plainly wrong. \
Each bullet is one concrete line that stands alone. State nothing the evidence does not support. Leave everything else alone; [] when nothing changes.";

pub fn prompt_for(question: &str, bullets: &[String], evidence: &[MemoryEntry]) -> String {
    let current = if bullets.is_empty() { "(empty)".to_string() } else { bullets.iter().enumerate().map(|(i, b)| format!("{}. {b}", i + 1)).collect::<Vec<_>>().join("\n") };
    let shown = evidence.iter().enumerate().map(|(i, e)| format!("{}. [{}] {}", i + 1, super::evolve::provenance_label(&e.kind), e.text.replace('\n', " "))).collect::<Vec<_>>().join("\n");
    format!("QUESTION: {question}\n\nCURRENT:\n{current}\n\nEVIDENCE:\n{shown}")
}

/// The new bullets after the model's operations, with secrets redacted. The same rules as the profile: dedupe, oldest give way when full.
pub fn apply(bullets: &[String], ops: &[serde_json::Value]) -> Vec<String> {
    let redacted: Vec<serde_json::Value> = ops
        .iter()
        .map(|op| {
            let mut op = op.clone();
            if let Some(t) = op.get("text").and_then(|v| v.as_str()).map(super::redact_secrets) {
                op["text"] = serde_json::Value::String(t);
            }
            op
        })
        .collect();
    super::evolve::apply_profile_delta(bullets, &redacted)
}

async fn db<T: Send + 'static>(root: &str, f: impl FnOnce(&Connection) -> Result<T, String> + Send + 'static) -> Result<T, String> {
    let root = root.to_string();
    tokio::task::spawn_blocking(move || super::store::with_conn(&root, f)).await.map_err(|e| format!("db task panicked: {e}"))?
}

/// The evidence nearest a question: observations first, then facts and notes, never pages.
async fn gather(root: &str, question: &str, auth: &std::sync::Arc<crate::ai::auth::AuthState>) -> Vec<MemoryEntry> {
    let mut found: Vec<MemoryEntry> = super::store::search_fts(root, question, EVIDENCE * 2).await.unwrap_or_default();
    if let Ok(Some(v)) = crate::ai::embedding::embed_query(Some(auth.as_ref()), question).await {
        for (e, _) in super::store::search_vector_scored(root, &v, EVIDENCE * 2).await.unwrap_or_default() {
            if !found.iter().any(|f| f.id == e.id) {
                found.push(e);
            }
        }
    }
    found.retain(|e| e.kind != "page");
    // Beliefs summarise facts, so they come first; each group keeps the order it was found in.
    found.sort_by_key(|e| e.kind != "observation");
    found.truncate(EVIDENCE);
    found
}

/// Refresh the pages whose evidence changed. Returns how many were rewritten.
pub async fn refresh(
    app: &tauri::AppHandle,
    root: &str,
    model: &crate::agent::runtime::types::ModelRef,
    auth: &std::sync::Arc<crate::ai::auth::AuthState>,
    defs: &[PageDef],
) -> usize {
    let keep: Vec<String> = defs.iter().map(|d| d.name.clone()).collect();
    for (name, entry_id) in db(root, {
        let keep = keep.clone();
        move |c| Ok(orphans(c, &keep))
    })
    .await
    .unwrap_or_default()
    {
        let _ = super::store::delete_entry(root, &entry_id).await;
        let _ = db(root, move |c| forget_row(c, &name)).await;
    }
    let mut refreshed = 0;
    for def in defs {
        if refreshed >= MAX_REFRESH_PER_PASS {
            break;
        }
        let evidence = gather(root, &def.question, auth).await;
        if evidence.is_empty() {
            continue;
        }
        let key = evidence_key(&evidence);
        let name = def.name.clone();
        let existing = db(root, move |c| Ok(row(c, &name))).await.ok().flatten();
        if existing.as_ref().is_some_and(|(_, k)| *k == key) {
            continue;
        }
        let current = match &existing {
            Some((id, _)) => super::store::get_entries_by_ids(root, std::slice::from_ref(id)).await.ok().and_then(|v| v.into_iter().next()),
            None => None,
        };
        let bullets = current.as_ref().map(|e| bullets_of(&e.text)).unwrap_or_default();
        let prompt = prompt_for(&def.question, &bullets, &evidence);
        // A failed call leaves the page and its fingerprint alone: the next pass tries again.
        let Ok(reply) = crate::ai::completion::complete_background(app, model, PAGE_PROMPT, &prompt).await else { continue };
        let Some(ops) = crate::ai::completion::extract_json_array(&reply).and_then(|v| v.as_array().cloned()) else { continue };
        let next = apply(&bullets, &ops);
        let now = super::now_ms();
        let entry_id = match (&current, next.is_empty()) {
            (_, true) => None,
            (Some(e), false) => {
                let text = render(&def.name, &next);
                if text != e.text {
                    let _ = super::store::update_entry_text(root, &e.id, &text).await;
                    embed(root, auth, &e.id, &text).await;
                    refreshed += 1;
                }
                Some(e.id.clone())
            }
            (None, false) => {
                let text = render(&def.name, &next);
                let entry = MemoryEntry {
                    id: uuid::Uuid::new_v4().to_string(),
                    key: format!("page_{}", super::slug(&def.name)),
                    text: text.clone(),
                    kind: "page".to_string(),
                    session_id: None,
                    created_at: now,
                    confidence: 1.0,
                    access_count: 0,
                    last_accessed_at: 0,
                    embedding_model: None,
                    user_wide: false,
                };
                if super::store::insert_entry(root, &entry).await.is_err() {
                    continue;
                }
                embed(root, auth, &entry.id, &text).await;
                refreshed += 1;
                Some(entry.id)
            }
        };
        // Remember what it was built from, so an unchanged question costs nothing next time.
        if let Some(id) = entry_id {
            let name = def.name.clone();
            let _ = db(root, move |c| put_row(c, &name, &id, &key, now)).await;
        }
    }
    refreshed
}

async fn embed(root: &str, auth: &std::sync::Arc<crate::ai::auth::AuthState>, id: &str, text: &str) {
    if let Ok(Some(v)) = crate::ai::embedding::embed_text(auth, text).await {
        let _ = super::store::update_embedding(root, id, &v, crate::ai::embedding::embedding_model()).await;
    }
}

/// The line the always-on block carries: which pages exist, not what they say.
pub fn titles_line(defs: &[PageDef]) -> Option<String> {
    if defs.is_empty() {
        return None;
    }
    let names = defs.iter().take(8).map(|d| d.name.as_str()).collect::<Vec<_>>().join("; ");
    Some(format!("Pages kept for this project (their content is found with recall): {names}."))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(id: &str, kind: &str, text: &str) -> MemoryEntry {
        MemoryEntry { id: id.into(), key: id.into(), text: text.into(), kind: kind.into(), session_id: None, created_at: 0, confidence: 1.0, access_count: 0, last_accessed_at: 0, embedding_model: None, user_wide: false }
    }
    fn def(n: &str, q: &str) -> PageDef {
        PageDef { name: n.into(), question: q.into() }
    }

    #[test]
    fn definitions_are_trimmed_and_checked() {
        let ok = clean_defs(vec![def("  Releases ", " How do we release? ")]).unwrap();
        assert_eq!(ok, vec![def("Releases", "How do we release?")]);
        assert!(clean_defs(vec![def("", "q")]).is_err());
        assert!(clean_defs(vec![def("a", "  ")]).is_err());
        assert!(clean_defs(vec![def("Releases", "q"), def("releases", "other")]).unwrap_err().contains("two pages"));
        assert!(clean_defs(vec![def(&"n".repeat(61), "q")]).is_err());
        assert!(clean_defs((0..13).map(|i| def(&format!("p{i}"), "q")).collect()).is_err());
        assert!(clean_defs((0..12).map(|i| def(&format!("p{i}"), "q")).collect()).is_ok());
    }

    #[test]
    fn the_fingerprint_changes_with_the_evidence_and_with_nothing_else() {
        let a = vec![e("1", "fact", "uses vite"), e("2", "observation", "ships fridays")];
        assert_eq!(evidence_key(&a), evidence_key(&a.clone()));
        let mut changed = a.clone();
        changed[1].text = "ships thursdays".to_string();
        assert_ne!(evidence_key(&a), evidence_key(&changed), "a changed text");
        assert_ne!(evidence_key(&a), evidence_key(&a[..1]), "less evidence");
        let mut swapped = a.clone();
        swapped.swap(0, 1);
        assert_ne!(evidence_key(&a), evidence_key(&swapped), "order is part of it: the nearest comes first");
        let mut touched = a.clone();
        touched[0].access_count = 9;
        assert_eq!(evidence_key(&a), evidence_key(&touched), "being recalled is not new evidence");
        assert_eq!(evidence_key(&[]).len(), 16);
    }

    #[test]
    fn a_page_is_a_title_and_bullets_and_reads_back() {
        let text = render("Releases", &["Tag from main".to_string(), "Run the installer build".to_string()]);
        assert_eq!(text, "Releases\n- Tag from main\n- Run the installer build");
        assert_eq!(bullets_of(&text), vec!["Tag from main", "Run the installer build"]);
        assert!(bullets_of("Only a title").is_empty());
    }

    #[test]
    fn deltas_edit_in_place_and_redact() {
        let key = format!("sk-{}", "Ab3dE6gH9jK2mN5pQ8sT1vW4yZ7".chars().take(26).collect::<String>());
        let ops = vec![
            serde_json::json!({"op": "edit", "n": 1, "text": "Tag from main, then push the tag"}),
            serde_json::json!({"op": "add", "text": format!("Publish with {key}")}),
            serde_json::json!({"op": "remove", "n": 2}),
        ];
        let next = apply(&["Tag from main".to_string(), "Upload by hand".to_string()], &ops);
        assert_eq!(next, vec!["Tag from main, then push the tag".to_string(), "Publish with [REDACTED:openai_key]".to_string()]);
        assert_eq!(apply(&["a".to_string()], &[]), vec!["a".to_string()], "no change is no change");
    }

    #[test]
    fn the_prompt_shows_the_question_the_bullets_and_numbered_labelled_evidence() {
        let p = prompt_for("How do we release?", &["Tag from main".to_string()], &[e("1", "observation", "Ships on fridays"), e("2", "fact", "Use the\ninstaller")]);
        assert!(p.starts_with("QUESTION: How do we release?\n\nCURRENT:\n1. Tag from main\n\nEVIDENCE:\n1. [belief drawn from several facts] Ships on fridays\n2. [saved on purpose] Use the installer"), "{p}");
        assert!(prompt_for("q", &[], &[]).contains("CURRENT:\n(empty)"));
    }

    #[test]
    fn rows_remember_the_fingerprint_and_orphans_are_found_by_name() {
        let c = Connection::open_in_memory().unwrap();
        init(&c).unwrap();
        assert_eq!(row(&c, "Releases"), None);
        put_row(&c, "Releases", "m1", "aaaa", 1).unwrap();
        put_row(&c, "Conventions", "m2", "bbbb", 1).unwrap();
        put_row(&c, "Releases", "m1", "cccc", 2).unwrap();
        assert_eq!(row(&c, "Releases"), Some(("m1".to_string(), "cccc".to_string())));
        assert_eq!(orphans(&c, &["Releases".to_string()]), vec![("Conventions".to_string(), "m2".to_string())]);
        forget_row(&c, "Conventions").unwrap();
        assert!(orphans(&c, &["Releases".to_string()]).is_empty());
    }

    #[test]
    fn only_the_names_go_into_the_always_on_block() {
        assert_eq!(titles_line(&[]), None);
        let line = titles_line(&[def("Releases", "How do we release? Secret detail"), def("Conventions", "q")]).unwrap();
        assert_eq!(line, "Pages kept for this project (their content is found with recall): Releases; Conventions.");
        assert!(!line.contains("Secret"));
    }
}

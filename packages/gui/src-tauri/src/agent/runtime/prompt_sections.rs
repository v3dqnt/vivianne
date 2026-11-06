//! Prompt sections: extra text added to the system prompt, in order, with variables.
//!
//! Sections come from settings (`promptSections`: a list of `{ id, order, text }`)
//! and from a per-thread scope, which inherits a global one. A thread's section
//! replaces a global or settings one with the same id. Text may use `{{cwd}}`,
//! `{{model}}`, `{{provider}}`, `{{project}}` and `{{date}}`.
//!
//! A section that uses a variable that doesn't exist is left out and reported,
//! rather than sending a prompt with a hole in it.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use serde_json::Value;

use crate::agent::scope::Scope;

#[derive(Debug, Clone, PartialEq)]
pub struct Section {
    pub order: i32,
    pub text: String,
}

fn global() -> &'static Scope<Section> {
    static G: OnceLock<Scope<Section>> = OnceLock::new();
    G.get_or_init(Scope::root)
}

fn threads() -> &'static Mutex<HashMap<String, Scope<Section>>> {
    static T: OnceLock<Mutex<HashMap<String, Scope<Section>>>> = OnceLock::new();
    T.get_or_init(Default::default)
}

/// A thread's own scope, under the global one.
pub fn thread_scope(thread: &str) -> Scope<Section> {
    threads().lock().unwrap_or_else(|e| e.into_inner()).entry(thread.to_string()).or_insert_with(|| global().child()).clone()
}

pub fn set(thread: Option<&str>, id: &str, order: i32, text: &str) {
    let section = Section { order, text: text.to_string() };
    match thread {
        Some(t) => thread_scope(t).register(id, section),
        None => global().register(id, section),
    }
}

pub fn remove(thread: Option<&str>, id: &str) -> bool {
    match thread {
        Some(t) => thread_scope(t).unregister(id),
        None => global().unregister(id),
    }
}

/// Free a thread's scope when it goes away.
pub fn forget(thread: &str) {
    if let Some(s) = threads().lock().unwrap_or_else(|e| e.into_inner()).remove(thread) {
        s.dispose();
    }
}

fn from_settings(settings: &Value) -> Vec<(String, Section)> {
    settings
        .get("promptSections")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|s| {
            let id = s.get("id")?.as_str()?.to_string();
            let text = s.get("text")?.as_str()?.to_string();
            Some((id, Section { order: s.get("order").and_then(Value::as_i64).unwrap_or(0) as i32, text }))
        })
        .collect()
}

/// Replace each `{{name}}` with its value. `Err` names the first variable that doesn't exist.
pub fn fill(text: &str, vars: &HashMap<&str, String>) -> Result<String, String> {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find("}}") else {
            out.push_str(&rest[start..]);
            return Ok(out);
        };
        let name = after[..end].trim();
        match vars.get(name) {
            Some(v) => out.push_str(v),
            None => return Err(name.to_string()),
        }
        rest = &after[end + 2..];
    }
    out.push_str(rest);
    Ok(out)
}

/// The sections for a thread as one block, in order, and what was left out and why.
pub fn assemble(settings: &Value, thread: Option<&str>, vars: &HashMap<&str, String>) -> (String, Vec<String>) {
    let mut all: Vec<(String, Section)> = from_settings(settings);
    let visible = match thread {
        Some(t) => thread_scope(t).visible(),
        None => global().visible(),
    };
    for (id, section) in visible {
        match all.iter_mut().find(|(k, _)| *k == id) {
            Some(slot) => slot.1 = section,
            None => all.push((id, section)),
        }
    }
    all.sort_by(|a, b| a.1.order.cmp(&b.1.order).then_with(|| a.0.cmp(&b.0)));
    let mut parts = vec![];
    let mut problems = vec![];
    for (id, section) in all {
        match fill(&section.text, vars) {
            Ok(text) if !text.trim().is_empty() => parts.push(text.trim().to_string()),
            Ok(_) => {}
            Err(name) => problems.push(format!("section `{id}` was left out: it uses the unknown variable `{name}`")),
        }
    }
    (parts.join("\n\n"), problems)
}

#[tauri::command]
pub async fn cmd_prompt_section_set(#[allow(non_snake_case)] threadId: Option<String>, id: String, order: Option<i32>, text: String) -> Result<(), String> {
    if id.trim().is_empty() {
        return Err("A section needs an id.".into());
    }
    set(threadId.as_deref(), id.trim(), order.unwrap_or(0), &text);
    Ok(())
}

#[tauri::command]
pub async fn cmd_prompt_section_remove(#[allow(non_snake_case)] threadId: Option<String>, id: String) -> Result<bool, String> {
    Ok(remove(threadId.as_deref(), &id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn vars() -> HashMap<&'static str, String> {
        HashMap::from([("cwd", "/work".to_string()), ("model", "m1".to_string())])
    }

    #[test]
    fn variables_are_filled_and_an_unknown_one_is_named() {
        assert_eq!(fill("in {{cwd}} using {{ model }}.", &vars()).unwrap(), "in /work using m1.");
        assert_eq!(fill("no variables", &vars()).unwrap(), "no variables");
        assert_eq!(fill("x {{nope}} y", &vars()), Err("nope".to_string()));
        assert_eq!(fill("an open {{ brace", &vars()).unwrap(), "an open {{ brace");
    }

    #[test]
    fn sections_come_out_in_order_and_a_thread_section_replaces_a_settings_one() {
        let settings = json!({ "promptSections": [
            { "id": "b", "order": 2, "text": "second" },
            { "id": "a", "order": 1, "text": "first in {{cwd}}" },
            { "id": "c", "order": 3, "text": "from settings" }
        ] });
        set(Some("sec-t1"), "c", 3, "from the thread");
        set(Some("sec-t1"), "d", 0, "zeroth");
        let (text, problems) = assemble(&settings, Some("sec-t1"), &vars());
        assert!(problems.is_empty());
        assert_eq!(text, "zeroth\n\nfirst in /work\n\nsecond\n\nfrom the thread");
        let (other, _) = assemble(&settings, Some("sec-t2"), &vars());
        assert_eq!(other, "first in /work\n\nsecond\n\nfrom settings", "another thread sees none of it");
        forget("sec-t1");
    }

    #[test]
    fn a_section_with_an_unknown_variable_is_left_out_and_reported_and_the_rest_stay() {
        let settings = json!({ "promptSections": [{ "id": "bad", "order": 1, "text": "uses {{missing}}" }, { "id": "ok", "order": 2, "text": "fine" }] });
        let (text, problems) = assemble(&settings, Some("sec-t3"), &vars());
        assert_eq!(text, "fine");
        assert_eq!(problems, ["section `bad` was left out: it uses the unknown variable `missing`"]);
    }

    #[test]
    fn a_global_section_reaches_every_thread_and_forgetting_a_thread_drops_its_own() {
        set(None, "sec-global", 5, "for everyone");
        set(Some("sec-t4"), "mine", 6, "just mine");
        let (a, _) = assemble(&json!({}), Some("sec-t4"), &vars());
        let (b, _) = assemble(&json!({}), Some("sec-t5"), &vars());
        assert!(a.contains("for everyone") && a.contains("just mine"));
        assert!(b.contains("for everyone") && !b.contains("just mine"));
        forget("sec-t4");
        assert!(!assemble(&json!({}), Some("sec-t4"), &vars()).0.contains("just mine"));
        assert!(remove(None, "sec-global") && !remove(None, "sec-global"));
    }
}

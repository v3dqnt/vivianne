//! "Done" means checked: what a run changed, and whether anything looked at
//! it afterwards.
//!
//! Agents often report success on work they never checked (arXiv
//! 2606.09863 found most agent failures were confident false "done"
//! claims, and that an LLM judge spots them poorly while cheap
//! deterministic checks do well). So this is deterministic: each change a
//! run makes (a file written, text typed, a click in an app or a browser)
//! stays "unchecked" until something observes it afterwards: reading the
//! file back, running a command (a build, the tests, the program), a
//! snapshot or a screenshot. When a run is about to end with unchecked
//! changes, or with todo items still open, the loop asks once: check them,
//! or say plainly that they're unchecked.

use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Domain {
    Files,
    Desktop,
    Browser,
}

#[derive(Debug, Default)]
pub struct Verification {
    /// Changed and not looked at since: (domain, what).
    pending: BTreeSet<(Domain, String)>,
    reminded: bool,
}

fn s<'a>(args: Option<&'a serde_json::Value>, key: &str) -> Option<&'a str> {
    args.and_then(|a| a.get(key)).and_then(|v| v.as_str())
}

impl Verification {
    /// Record one finished tool call.
    pub fn observe(&mut self, tool: &str, args: Option<&serde_json::Value>, failed: bool, output: &str) {
        let action = s(args, "action").unwrap_or("");
        // Observations first: they clear what they look at.
        match (tool, action) {
            ("read" | "grep" | "glob", _) => {
                if let Some(path) = s(args, "path") {
                    let path = norm_path(path);
                    self.pending.retain(|(d, w)| !(*d == Domain::Files && norm_path(w) == path));
                }
                if tool == "grep" {
                    // A search over the project looks at everything.
                    if s(args, "path").map_or(true, |p| !p.contains('.')) {
                        self.clear(Domain::Files);
                    }
                }
            }
            // Running anything (a build, the tests, the program) exercises
            // the changes made so far.
            ("bash" | "powershell" | "run_script" | "diagnostics" | "sandbox_execute", _) if !failed => self.clear(Domain::Files),
            ("dip" | "computer", "read_text" | "snapshot" | "screenshot" | "find" | "zoom") if !failed => self.clear(Domain::Desktop),
            ("user_browser_driver", "snapshot" | "read" | "screenshot" | "find" | "read_console") if !failed => self.clear(Domain::Browser),
            ("browser_snapshot" | "web_preview", _) if !failed => self.clear(Domain::Browser),
            _ => {}
        }
        if failed {
            return;
        }
        // Changes.
        let change = match (tool, action) {
            ("write" | "edit" | "notebook_edit" | "edit_document" | "create_document", _) => {
                s(args, "path").or_else(|| s(args, "file_path")).map(|p| (Domain::Files, p.to_string()))
            }
            ("apply_patch", _) => Some((Domain::Files, "the patched files".to_string())),
            ("dip" | "computer", "type") if output.contains(crate::coding_agent::computer::VERIFIED_MARK) => None,
            ("dip" | "computer", "type") => Some((Domain::Desktop, "the text you typed".to_string())),
            ("dip" | "computer", "key" | "left_click" | "double_click" | "triple_click" | "right_click" | "left_click_drag") => {
                Some((Domain::Desktop, "your clicks and key presses in the app".to_string()))
            }
            ("dip" | "computer", "interact") => Some((Domain::Desktop, "what you changed in the app".to_string())),
            ("user_browser_driver", "click" | "type" | "select" | "key" | "evaluate") => Some((Domain::Browser, "what you did in the browser".to_string())),
            ("browser_click" | "browser_type" | "browser_press_key", _) => Some((Domain::Browser, "what you did in the browser".to_string())),
            _ => None,
        };
        if let Some(c) = change {
            self.pending.insert(c);
        }
    }

    fn clear(&mut self, domain: Domain) {
        self.pending.retain(|(d, _)| *d != domain);
    }

    /// The reminder to send before the run ends, once per run. `open_todos`
    /// are the todo items not yet completed.
    pub fn reminder(&mut self, open_todos: &[String]) -> Option<String> {
        if self.reminded || (self.pending.is_empty() && open_todos.is_empty()) {
            return None;
        }
        self.reminded = true;
        let mut text = String::from("Before you finish:");
        if !self.pending.is_empty() {
            let what: Vec<String> = self.pending.iter().map(|(_, w)| w.clone()).take(8).collect();
            text.push_str(&format!(
                " you changed {} and haven't checked since. Check it now (read it back, run it or the tests, take a snapshot or screenshot), \
                 or say plainly in your reply that it is unchecked.",
                what.join(", ")
            ));
        }
        if !open_todos.is_empty() {
            let items: Vec<String> = open_todos.iter().take(8).cloned().collect();
            text.push_str(&format!(
                " Your todo list still has open items: {}. Finish them, or say which are left and why.",
                items.join("; ")
            ));
        }
        text.push_str(" Don't describe anything as done that you haven't seen working.");
        Some(text)
    }
}

fn norm_path(p: &str) -> String {
    p.replace('\\', "/").trim_start_matches("./").to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn edits_need_a_look_before_done() {
        let mut v = Verification::default();
        v.observe("edit", Some(&json!({ "path": "src/a.rs" })), false, "");
        v.observe("write", Some(&json!({ "path": "src/b.rs" })), false, "");
        v.observe("read", Some(&json!({ "path": "./src/a.rs" })), false, "");
        let r = v.reminder(&[]).unwrap();
        assert!(r.contains("src/b.rs") && !r.contains("src/a.rs"), "{r}");
        assert!(v.reminder(&[]).is_none(), "once per run");
    }

    #[test]
    fn running_something_checks_file_changes() {
        let mut v = Verification::default();
        v.observe("edit", Some(&json!({ "path": "src/a.rs" })), false, "");
        v.observe("bash", Some(&json!({ "command": "cargo test" })), false, "");
        assert!(v.reminder(&[]).is_none());
    }

    #[test]
    fn typing_is_checked_by_its_own_read_back_or_a_snapshot() {
        let mut v = Verification::default();
        v.observe("dip", Some(&json!({ "action": "type" })), false, "Typed 5. Read back: the field contains it [verified].");
        assert!(v.reminder(&[]).is_none(), "verified in the tool");

        let mut v = Verification::default();
        v.observe("dip", Some(&json!({ "action": "type" })), false, "Typed 5.");
        v.observe("dip", Some(&json!({ "action": "read_text" })), false, "hello");
        assert!(v.reminder(&[]).is_none(), "checked with read_text");

        let mut v = Verification::default();
        v.observe("dip", Some(&json!({ "action": "type" })), false, "Typed 5. Read back: MISMATCH");
        assert!(v.reminder(&[]).unwrap().contains("typed"));
    }

    #[test]
    fn failed_calls_change_nothing_and_reads_dont_need_checking() {
        let mut v = Verification::default();
        v.observe("edit", Some(&json!({ "path": "a.rs" })), true, "");
        v.observe("read", Some(&json!({ "path": "b.rs" })), false, "");
        v.observe("web_search", Some(&json!({ "query": "x" })), false, "");
        assert!(v.reminder(&[]).is_none());
    }

    #[test]
    fn open_todos_are_named() {
        let mut v = Verification::default();
        let r = v.reminder(&["write the tests".to_string()]).unwrap();
        assert!(r.contains("write the tests"));
    }
}

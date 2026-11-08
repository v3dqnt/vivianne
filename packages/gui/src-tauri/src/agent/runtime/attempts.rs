//! What the run has tried, kept by the loop rather than by the model.
//!
//! The doom-loop check only sees identical calls in a row; a model that
//! keeps retrying one failing idea with small variations (quoting changed,
//! a flag moved, `./src/a.rs` for `src/a.rs`) walks past it. So every call
//! is recorded under a normalized key with its outcome, and when the same
//! thing has failed before, the model is told — as data: what was tried,
//! how many times, and the error each time — rather than nudged in prose.
//!
//! It lives outside the message list, so compaction and masking can't take
//! it away: the failures are still known after the outputs are gone.

use std::collections::BTreeMap;

/// Failed attempts at one key before the model is shown the record.
const FAILURES_BEFORE_NOTE: u32 = 2;
/// Errors kept per key, newest last.
const ERRORS_KEPT: usize = 3;
/// Error text kept per failure.
const ERROR_CHARS: usize = 240;
/// Keys listed in a note.
const NOTE_KEYS: usize = 6;

#[derive(Debug, Default, Clone)]
struct Entry {
    /// How it reads in a note: `bash: cargo test -p app`.
    label: String,
    tries: u32,
    failures: u32,
    errors: Vec<String>,
    last_step: usize,
    /// Whether the model has already been shown this key's failures at the
    /// current count, so one failure isn't announced every turn.
    noted_at: u32,
}

#[derive(Debug, Default)]
pub struct Attempts {
    entries: BTreeMap<String, Entry>,
}

/// One argument string, compared loosely: whitespace collapsed, quotes and
/// a leading `./` dropped, slashes one way, case folded.
fn loose(s: &str) -> String {
    let s = s.replace('\\', "/").replace(['"', '\''], "");
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    let s = s.strip_prefix("./").unwrap_or(&s).to_string();
    s.to_lowercase()
}

/// The argument that says what a call is about, for the tools where one does.
fn subject(tool: &str, args: &serde_json::Value) -> Option<String> {
    let arg = |k: &str| args.get(k).and_then(|v| v.as_str()).map(loose);
    match tool {
        "bash" | "powershell" | "streaming_bash" => arg("command"),
        "read" | "write" | "edit" | "multi_edit" | "ls" => arg("path").or_else(|| arg("file_path")),
        "grep" | "glob" | "find" => {
            let p = arg("pattern").unwrap_or_default();
            let at = arg("path").unwrap_or_default();
            Some(format!("{p} @ {at}"))
        }
        "web_fetch" => arg("url"),
        "web_search" | "code_search" => arg("query"),
        _ => None,
    }
}

/// The key a call is filed under, and its label.
pub fn key_of(tool: &str, args: &serde_json::Value) -> (String, String) {
    match subject(tool, args) {
        Some(s) => (format!("{tool}\u{1f}{s}"), format!("{tool}: {}", clip(&s, 120))),
        None => {
            // Any other tool: its whole arguments, loosely.
            let flat = loose(&args.to_string());
            (format!("{tool}\u{1f}{flat}"), format!("{tool} {}", clip(&flat, 120)))
        }
    }
}

fn clip(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(n).collect::<String>())
    }
}

/// The part of an error worth repeating: its first non-empty lines.
fn error_head(text: &str) -> String {
    let lines: Vec<&str> = text.lines().map(str::trim).filter(|l| !l.is_empty()).take(4).collect();
    clip(&lines.join(" | "), ERROR_CHARS)
}

impl Attempts {
    /// File one finished call.
    pub fn record(&mut self, step: usize, tool: &str, args: &serde_json::Value, failed: bool, output: &str) {
        let (key, label) = key_of(tool, args);
        let e = self.entries.entry(key).or_default();
        e.label = label;
        e.tries += 1;
        e.last_step = step;
        if failed {
            e.failures += 1;
            e.errors.push(error_head(output));
            if e.errors.len() > ERRORS_KEPT {
                e.errors.remove(0);
            }
        } else {
            // It works now: what failed before is no longer the story.
            e.failures = 0;
            e.errors.clear();
            e.noted_at = 0;
        }
    }

    /// Calls just made that have failed before, at a count not yet shown:
    /// a note listing each with its errors. `None` when there's nothing new.
    pub fn note_for(&mut self, tools: &[(String, serde_json::Value)]) -> Option<String> {
        let mut keys: Vec<String> = Vec::new();
        for (tool, args) in tools {
            let (key, _) = key_of(tool, args);
            if let Some(e) = self.entries.get(&key) {
                if e.failures >= FAILURES_BEFORE_NOTE && e.noted_at < e.failures && !keys.contains(&key) {
                    keys.push(key);
                }
            }
        }
        if keys.is_empty() {
            return None;
        }
        let mut out = String::from("Attempt record (kept by the app, survives compaction). These have failed before:\n");
        for key in keys.iter().take(NOTE_KEYS) {
            let e = self.entries.get_mut(key).expect("listed above");
            e.noted_at = e.failures;
            out.push_str(&format!("- {} — failed {} of {} tries", e.label, e.failures, e.tries));
            for err in &e.errors {
                out.push_str(&format!("\n    error: {err}"));
            }
            out.push('\n');
        }
        out.push_str(
            "Don't run these again unchanged. Say what the errors have in common, then try something that addresses it, or stop and say what's blocking you.",
        );
        Some(out)
    }

    /// Everything that is still failing, for a briefing after compaction.
    pub fn failing_summary(&self) -> Option<String> {
        let mut failing: Vec<&Entry> = self.entries.values().filter(|e| e.failures > 0).collect();
        if failing.is_empty() {
            return None;
        }
        failing.sort_by_key(|e| std::cmp::Reverse(e.last_step));
        let mut out = String::from("Still failing from earlier in this run (kept by the app):\n");
        for e in failing.iter().take(NOTE_KEYS) {
            out.push_str(&format!("- {} — failed {}x", e.label, e.failures));
            if let Some(err) = e.errors.last() {
                out.push_str(&format!(": {err}"));
            }
            out.push('\n');
        }
        Some(out.trim_end().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn small_variations_file_under_one_key() {
        let a = key_of("bash", &json!({ "command": "cargo  test -p \"app\"" })).0;
        let b = key_of("bash", &json!({ "command": "cargo test -p app" })).0;
        assert_eq!(a, b);
        let c = key_of("read", &json!({ "path": "./src\\Lib.rs" })).0;
        let d = key_of("read", &json!({ "path": "src/lib.rs" })).0;
        assert_eq!(c, d);
    }

    #[test]
    fn a_second_failure_is_shown_once_with_its_errors() {
        let mut at = Attempts::default();
        let call = ("bash".to_string(), json!({ "command": "npm test" }));
        at.record(1, &call.0, &call.1, true, "Error: cannot find module 'x'\n  at a.js:1");
        assert!(at.note_for(std::slice::from_ref(&call)).is_none(), "one failure is not a pattern");
        at.record(2, &call.0, &call.1, true, "Error: cannot find module 'x'");
        let note = at.note_for(std::slice::from_ref(&call)).expect("two failures are");
        assert!(note.contains("failed 2 of 2 tries") && note.contains("cannot find module"));
        assert!(at.note_for(std::slice::from_ref(&call)).is_none(), "not repeated at the same count");
    }

    #[test]
    fn success_clears_the_record() {
        let mut at = Attempts::default();
        let call = ("bash".to_string(), json!({ "command": "make" }));
        at.record(1, &call.0, &call.1, true, "boom");
        at.record(2, &call.0, &call.1, true, "boom");
        at.record(3, &call.0, &call.1, false, "ok");
        assert!(at.note_for(std::slice::from_ref(&call)).is_none());
        assert!(at.failing_summary().is_none());
    }
}

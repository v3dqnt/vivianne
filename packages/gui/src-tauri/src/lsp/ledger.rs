//! What the agent has already been told about a file's errors, so a second edit to the same file does not repeat the same list.
//!
//! A diagnostic's identity is its severity, source, code and message, with any `path:line:col` the message begins with removed, so a message
//! that only moved down the file after an insert is still the same one. The ledger keeps, per file, the identities from the last report: an
//! error that was fixed and later comes back is new again.

use std::collections::{HashMap, HashSet};

use super::Diagnostic;

/// How many errors one report may list.
pub const MAX_LISTED: usize = 50;

/// The message without a leading `path:line:col:` (or `path(line,col):`) and without the file's own path anywhere in it.
fn strip_location(message: &str, file: &str) -> String {
    let m = message.replace(file, "<file>");
    let b = m.as_bytes();
    // `something.ext:12:3: text` or `something.ext(12,3): text` at the very start.
    let mut i = 0;
    while i < b.len() && !b[i].is_ascii_whitespace() && b[i] != b':' && b[i] != b'(' {
        i += 1;
    }
    let rest = &m[i..];
    let after = rest
        .strip_prefix(':')
        .map(|r| r.trim_start_matches(|c: char| c.is_ascii_digit() || c == ':'))
        .or_else(|| rest.strip_prefix('(').and_then(|r| r.split_once(')').map(|(_, t)| t.trim_start_matches(':'))));
    match after {
        // Only a real location prefix: the start must have looked like a file name and digits must have followed.
        Some(text) if i > 0 && m[..i].contains('.') && text.len() < rest.len() => text.trim().to_string(),
        _ => m.trim().to_string(),
    }
}

pub fn identity(file: &str, d: &Diagnostic) -> String {
    format!("{}|{}|{}|{}", d.severity.unwrap_or(1), d.source.as_deref().unwrap_or(""), d.code.as_ref().map(|c| c.to_string()).unwrap_or_default(), strip_location(&d.message, file))
}

/// What a report adds to what the agent already knows.
#[derive(Debug, Default, PartialEq)]
pub struct Review {
    /// Errors not in the previous report for this file.
    pub new: Vec<Diagnostic>,
    /// Errors that were already reported and are still there.
    pub repeated: usize,
}

#[derive(Default)]
pub struct Ledger {
    shown: HashMap<String, HashSet<String>>,
}

impl Ledger {
    /// Compare `items` (this file's current diagnostics) with the last report for `key`, and remember them. Only errors count, as only errors are shown.
    pub fn review(&mut self, key: &str, file: &str, items: &[Diagnostic]) -> Review {
        let errors: Vec<&Diagnostic> = items.iter().filter(|d| d.severity.unwrap_or(1) == 1).collect();
        let before = self.shown.remove(key).unwrap_or_default();
        let mut now: HashSet<String> = HashSet::new();
        let mut review = Review::default();
        for d in errors {
            let id = identity(file, d);
            // Two errors with one identity are one line to the reader.
            if !now.insert(id.clone()) {
                continue;
            }
            if before.contains(&id) {
                review.repeated += 1;
            } else {
                review.new.push(d.clone());
            }
        }
        if !now.is_empty() {
            self.shown.insert(key.to_string(), now);
        }
        review
    }

    /// Forget a file (it was deleted, or the agent wants the whole list again).
    pub fn forget(&mut self, key: &str) {
        self.shown.remove(key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lsp::{Position, Range};

    fn err(message: &str, line: u32) -> Diagnostic {
        Diagnostic { range: Range { start: Position { line, character: 0 }, end: Position { line, character: 1 } }, severity: Some(1), message: message.into(), source: Some("rustc".into()), code: None }
    }

    #[test]
    fn an_error_is_new_once_then_repeated_then_new_again_after_it_was_fixed() {
        let mut l = Ledger::default();
        let r = l.review("k", "/w/a.rs", &[err("cannot find value `x`", 3)]);
        assert_eq!((r.new.len(), r.repeated), (1, 0));
        // The same error, moved down by an insert above it.
        let r = l.review("k", "/w/a.rs", &[err("cannot find value `x`", 9), err("mismatched types", 10)]);
        assert_eq!((r.new.len(), r.repeated), (1, 1));
        assert_eq!(r.new[0].message, "mismatched types");
        // Fixed (a clean report), then it comes back.
        assert_eq!(l.review("k", "/w/a.rs", &[]), Review::default());
        assert_eq!(l.review("k", "/w/a.rs", &[err("mismatched types", 2)]).new.len(), 1);
    }

    #[test]
    fn a_location_at_the_start_of_the_message_is_not_part_of_its_identity() {
        let a = identity("/w/a.rs", &err("a.rs:12:5: expected `;`", 11));
        let b = identity("/w/a.rs", &err("a.rs:40:5: expected `;`", 39));
        assert_eq!(a, b);
        let c = identity("/w/a.rs", &err("src/a.rs(12,5): expected `;`", 11));
        assert_eq!(a, c);
        // Ordinary text with a colon is left alone.
        assert_ne!(identity("/w/a.rs", &err("note: expected `;`", 1)), identity("/w/a.rs", &err("warning: expected `;`", 1)));
    }

    #[test]
    fn warnings_do_not_count_and_files_are_separate() {
        let mut l = Ledger::default();
        let mut w = err("unused", 1);
        w.severity = Some(2);
        assert_eq!(l.review("k", "f", &[w]), Review::default());
        assert_eq!(l.review("a", "a", &[err("boom", 1)]).new.len(), 1);
        assert_eq!(l.review("b", "b", &[err("boom", 1)]).new.len(), 1, "another file's report is its own");
    }
}

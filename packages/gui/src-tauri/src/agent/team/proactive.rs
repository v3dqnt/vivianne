//! How a 24/7 agent acts on its own without becoming noise: how often it may
//! suggest something nobody asked for, when it stays quiet, what it never
//! raises again, and the lines it never crosses without an explicit yes.
//!
//! Pure rules here; `bots` does the file and clock work around them.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Unprompted suggestions a day, per agent.
pub const DAILY_SUGGESTIONS: u32 = 3;
/// After this many suggestions in a row go unanswered, it slows down.
const IGNORED_BEFORE_SLOWING: u32 = 2;
const SLOW_FOR_MS: u64 = 7 * 24 * 3_600_000;
/// Quiet from 22:00 to 08:00 local time; what it found waits for the morning brief.
const QUIET_FROM: u32 = 22;
const MORNING: u32 = 8;
const NOON: u32 = 12;

/// What the user did with a suggestion card.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Answer {
    Yes,
    Later,
    Never,
}

/// One agent's record of its own suggestions, kept beside its settings.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pacing {
    /// The local day `suggested_today` counts for (YYYY-MM-DD).
    #[serde(default)]
    pub day: String,
    #[serde(default)]
    pub suggested_today: u32,
    #[serde(default)]
    pub ignored_in_row: u32,
    #[serde(default)]
    pub slow_until: u64,
    /// Suggestions still waiting for an answer.
    #[serde(default)]
    pub open: Vec<String>,
    /// Every answered suggestion, so its card stays stamped.
    #[serde(default)]
    pub answered: HashMap<String, Answer>,
    /// The day the last morning brief went out.
    #[serde(default)]
    pub brief_day: String,
}

impl Pacing {
    pub fn is_quiet(hour: u32) -> bool {
        hour >= QUIET_FROM || hour < MORNING
    }

    fn daily_limit(&self, now: u64) -> u32 {
        if now < self.slow_until { DAILY_SUGGESTIONS / 2 } else { DAILY_SUGGESTIONS }
    }

    /// Whether it may look for something to suggest now.
    pub fn may_suggest(&self, day: &str, hour: u32, now: u64) -> bool {
        let used = if self.day == day { self.suggested_today } else { 0 };
        !Self::is_quiet(hour) && used < self.daily_limit(now)
    }

    /// Once a day, in the morning: an agent switched on in the afternoon
    /// waits for tomorrow's.
    pub fn brief_due(&self, day: &str, hour: u32) -> bool {
        (MORNING..NOON).contains(&hour) && self.brief_day != day
    }

    /// It posted a suggestion. One still open when the next arrives was
    /// ignored; enough of those in a row and it slows down for a week.
    pub fn suggested(&mut self, text: &str, day: &str, now: u64) {
        if self.day != day {
            self.day = day.to_string();
            self.suggested_today = 0;
        }
        self.suggested_today += 1;
        if !self.open.is_empty() {
            self.ignored_in_row += 1;
            self.open.clear();
        }
        if self.ignored_in_row >= IGNORED_BEFORE_SLOWING {
            self.slow_until = now + SLOW_FOR_MS;
            self.ignored_in_row = 0;
        }
        self.open.push(text.to_string());
    }

    pub fn answer(&mut self, text: &str, answer: Answer) {
        self.open.retain(|t| t != text);
        self.ignored_in_row = 0;
        self.answered.insert(text.to_string(), answer);
    }

    /// What the user said never to suggest again.
    pub fn never(&self) -> Vec<&str> {
        self.answered.iter().filter(|(_, a)| **a == Answer::Never).map(|(t, _)| t.as_str()).collect()
    }
}

/// The suggestion an agent's reply ends with: a line starting `SUGGEST:`.
pub fn suggestion_in(reply: &str) -> Option<String> {
    reply.lines().find_map(|l| l.trim().strip_prefix("SUGGEST:").map(|s| s.trim().to_string())).filter(|s| !s.is_empty())
}

/// A line an agent never crosses on its own, whatever its rules say: the
/// call needs the user's explicit yes. `None` for everything else.
///
/// ponytail: a word scan of the tool and its arguments; it errs towards
/// asking. Swap in per-tool checks if it asks too often.
pub fn hard_line(tool: &str, args: &serde_json::Value) -> Option<&'static str> {
    let action = args.get("action").and_then(|a| a.as_str()).unwrap_or("");
    if matches!((tool, action), ("email", "send") | ("phone", "send_sms" | "call")) {
        return Some("contacting someone");
    }
    let text = format!("{tool} {args}").to_lowercase();
    let words: Vec<&str> = text.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect();
    let has = |list: &[&str]| words.iter().any(|w| list.contains(w));
    if matches!(tool, "bash" | "powershell" | "terminal") {
        let deletes = has(&["rm", "del", "erase", "rmdir", "unlink"]) || text.contains("remove-item");
        return deletes.then_some("deleting something");
    }
    if has(&["pay", "payment", "purchase", "checkout", "buy", "transfer", "donate", "card"]) {
        Some("spending money")
    } else if tool.starts_with("mcp__") && has(&["send", "post", "reply", "message", "tweet", "publish", "invite", "share"]) {
        Some("sending in your name")
    } else if has(&["delete", "trash"]) {
        Some("deleting something")
    } else if has(&["password", "passwords", "2fa", "mfa", "ssn", "passport"]) {
        Some("personal data or account security")
    } else if has(&["accept", "agree", "terms", "consent", "invitation"]) {
        Some("accepting terms or an invitation")
    } else if has(&["legal", "lawyer", "lawsuit", "medical", "doctor", "prescription", "diagnosis"]) {
        Some("a legal or medical matter")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn three_suggestions_a_day_then_quiet_until_tomorrow() {
        let mut p = Pacing::default();
        for _ in 0..3 {
            assert!(p.may_suggest("2026-10-30", 14, 0));
            p.suggested("x", "2026-10-30", 0);
            p.answer("x", Answer::Later);
        }
        assert!(!p.may_suggest("2026-10-30", 14, 0));
        assert!(p.may_suggest("2026-10-31", 9, 0));
    }

    #[test]
    fn nothing_unprompted_at_night_and_the_brief_comes_once_a_morning() {
        let p = Pacing::default();
        assert!(!p.may_suggest("d", 23, 0) && !p.may_suggest("d", 3, 0));
        assert!(p.brief_due("d", 8) && !p.brief_due("d", 7) && !p.brief_due("d", 15));
        let briefed = Pacing { brief_day: "d".into(), ..Pacing::default() };
        assert!(!briefed.brief_due("d", 9));
    }

    #[test]
    fn two_ignored_in_a_row_halves_the_rate_for_a_week() {
        let mut p = Pacing::default();
        p.suggested("a", "d", 0);
        p.suggested("b", "d", 0); // a ignored
        p.suggested("c", "e", 0); // b ignored
        assert_eq!(p.daily_limit(1), 1);
        assert_eq!(p.daily_limit(SLOW_FOR_MS + 1), DAILY_SUGGESTIONS);
    }

    #[test]
    fn an_answer_resets_the_streak_and_never_is_remembered() {
        let mut p = Pacing::default();
        p.suggested("a", "d", 0);
        p.answer("a", Answer::Never);
        p.suggested("b", "d", 0);
        assert_eq!(p.ignored_in_row, 0, "a was answered, not ignored");
        assert_eq!(p.never(), vec!["a"]);
    }

    #[test]
    fn the_suggestion_line_is_found() {
        assert_eq!(suggestion_in("Found a cheaper flight.\nSUGGEST: Rebook to the 9am flight"), Some("Rebook to the 9am flight".into()));
        assert_eq!(suggestion_in("All quiet."), None);
    }

    #[test]
    fn hard_lines_ask_whatever_the_rules() {
        assert!(hard_line("email", &json!({ "action": "send", "to": "a@b.c" })).is_some());
        assert!(hard_line("email", &json!({ "action": "reply" })).is_none(), "answering someone who wrote is not new contact");
        assert!(hard_line("user_browser_driver", &json!({ "action": "click", "target": "Pay now" })).is_some());
        assert!(hard_line("bash", &json!({ "command": "rm -rf build" })).is_some());
        assert!(hard_line("powershell", &json!({ "command": "Remove-Item notes.txt" })).is_some());
        assert!(hard_line("mcp__slack__send_message", &json!({ "text": "hi" })).is_some());
        assert!(hard_line("user_browser_driver", &json!({ "action": "click", "target": "Accept all cookies" })).is_some());
        assert!(hard_line("bash", &json!({ "command": "ls -la" })).is_none());
        assert!(hard_line("bash", &json!({ "command": "git add card.tsx" })).is_none(), "a shell only trips the deletion line");
        assert!(hard_line("user_browser_driver", &json!({ "action": "navigate", "url": "https://news.ycombinator.com" })).is_none());
        assert!(hard_line("mcp__notion__search", &json!({ "query": "roadmap" })).is_none());
    }
}

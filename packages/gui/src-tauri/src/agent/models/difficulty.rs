//! How hard a task is, so a model of the right size can take it.
//!
//! Routing by role (`routing::route`) sends background work to a small model.
//! This goes one step further for work that has a request in it, a subagent's
//! brief or a teammate's routine: read the request, call it easy, medium or
//! hard, and let the routing tiers pick a model for that level.
//!
//! Three deciders, tried in order, each optional:
//!
//! 1. **Laya**, the local decision engine, when it is installed and on. One
//!    forward pass of a small encoder, calibrated probabilities, no network.
//! 2. **Jev**, the hosted typed-decision model, when there is a key. Fast, but
//!    a round trip, so it has a tight budget and switches itself off if it
//!    keeps failing.
//! 3. **Rules**, always there: length, how many things are asked, and words
//!    that mean "think" or "just do".
//!
//! Anything uncertain falls through to the next. The result says which one
//! decided, so the UI can show it.

use std::time::Duration;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Difficulty {
    Easy,
    Medium,
    Hard,
}

impl Difficulty {
    pub fn parse(s: &str) -> Option<Difficulty> {
        match s.trim().to_ascii_lowercase().as_str() {
            "easy" => Some(Difficulty::Easy),
            "medium" => Some(Difficulty::Medium),
            "hard" => Some(Difficulty::Hard),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Verdict {
    pub level: Difficulty,
    /// "laya", "jev" or "rules": what decided.
    pub by: &'static str,
}

/// Words that mean the work needs thinking, not typing.
const HARD_WORDS: &[&str] = &[
    "architecture", "refactor", "redesign", "design", "root cause", "race condition", "deadlock", "performance", "optimi", "migrate", "security",
    "prove", "derive", "algorithm", "across the codebase", "end to end", "end-to-end", "investigate", "why does", "why is", "debug", "concurrency",
    "trade-off", "tradeoff", "plan the", "strategy", "multi-step", "reverse engineer",
];

/// Words that mean it is a small, bounded job.
const EASY_WORDS: &[&str] = &[
    "rename", "typo", "list ", "show me", "what is", "what's", "summarise", "summarize", "translate", "format", "sort ", "count ", "find the file",
    "where is", "open ", "read ", "tl;dr", "one line", "quick", "simple",
];

/// Verbs that ask for something to be made.
const MAKE_VERBS: &[&str] = &["create", "build", "make", "implement", "develop", "write", "design", "set up", "setup", "generate", "scaffold", "code"];

/// Things that are made of many parts. "A website" is not a one-liner.
const BIG_THINGS: &[&str] = &[
    "website", "web site", "web app", "webapp", "app", "application", "platform", "system", "game", "api", "backend", "back-end", "frontend", "front-end",
    "dashboard", "pipeline", "compiler", "framework", "service", "server", "database", "e-commerce", "ecommerce", "store", "portal", "extension", "plugin",
    "bot", "cli", "library", "engine", "interpreter", "simulator", "crm", "saas",
];

/// Pieces that make a build bigger.
const SCOPE_WORDS: &[&str] = &["auth", "login", "database", "tests", "deploy", "full-stack", "full stack", "responsive", "payments", "admin", "docker", "ci"];

/// Greetings, thanks and one-word acknowledgements: no task in them, so no
/// model is needed to see they are easy.
pub fn is_small_talk(text: &str) -> bool {
    let t: String = text.trim().to_lowercase().chars().filter(|c| c.is_alphanumeric() || c.is_whitespace()).collect();
    let words: Vec<&str> = t.split_whitespace().collect();
    if words.is_empty() {
        return true;
    }
    const TALK: &[&str] = &[
        "hi", "hello", "hey", "yo", "sup", "thanks", "thank", "you", "thx", "ok", "okay", "yes", "no", "yep", "nope", "sure", "cool", "nice", "great", "good", "morning",
        "evening", "afternoon", "night", "bye", "goodbye", "please", "lol", "hmm", "test", "testing", "there", "how", "are", "whats", "up", "again", "got", "it",
    ];
    words.len() <= 4 && words.iter().all(|w| TALK.contains(w))
}

/// The rules, with no I/O.
pub fn rules(text: &str) -> Difficulty {
    let t = text.trim().to_lowercase();
    if t.is_empty() || is_small_talk(&t) {
        return Difficulty::Easy;
    }
    let words = t.split_whitespace().count();
    // How many separate things are asked: sentences, list items, "and then".
    let asks = t.matches('?').count()
        + t.lines().filter(|l| l.trim_start().starts_with(['-', '*', '1', '2', '3', '4', '5'])).count()
        + t.matches(" and then ").count()
        + t.matches(" then ").count();
    let hard_hits = HARD_WORDS.iter().filter(|w| t.contains(*w)).count();
    let easy_hits = EASY_WORDS.iter().filter(|w| t.contains(*w)).count();

    let mut score: i32 = 0;
    // Asking for something to be built is at least a normal task, and
    // a whole website, app or system is a big one.
    let makes = MAKE_VERBS.iter().any(|v| t.split(|c: char| !c.is_alphanumeric() && c != '-').any(|w| w == *v) || t.contains(&format!("{v} a ")) || t.contains(&format!("{v} an ")));
    let big = BIG_THINGS.iter().any(|b| t.split(|c: char| !c.is_alphanumeric() && c != '-').any(|w| w == *b) || t.contains(&format!(" {b} ")));
    if makes && big {
        score += 2;
        score += SCOPE_WORDS.iter().filter(|w| t.contains(*w)).count().min(2) as i32;
    } else if makes {
        score += 1;
    }
    // A one-line ask is the baseline; most written-out tasks are a notch up.
    score += match words {
        0..=10 => 0,
        11..=80 => 1,
        81..=200 => 2,
        _ => 3,
    };
    score += (asks.min(4) as i32) / 2;
    score += hard_hits.min(3) as i32;
    score -= easy_hits.min(2) as i32;
    match score {
        i32::MIN..=0 => Difficulty::Easy,
        1..=2 => Difficulty::Medium,
        _ => Difficulty::Hard,
    }
}

/// The questions put to Laya. A three-way choice alone is weak for short
/// requests, so two yes/no questions go with it.
pub fn questions() -> serde_json::Value {
    serde_json::json!({
        "difficulty": {
            "type": "choice",
            "instructions": "How much work is this request for an AI agent that can read and write files and run commands?",
            "criteria": {
                "easy": "a greeting, a question with a short answer, or a tiny bounded job: a lookup, a rename, a summary, one file, one command",
                "medium": "a normal piece of work: a feature or fix in a few files, making a small page or script, a short investigation, several steps",
                "hard": "a large or demanding job: building a whole website, app or system, designing architecture, a subtle bug across many files, deep reasoning, many dependent steps"
            }
        },
        "build": {
            "type": "noul",
            "instructions": "Does the request ask for something to be created or built, such as a website, an app, a program, a document or a design?"
        },
        "talk": {
            "type": "noul",
            "instructions": "Is the request only a greeting, thanks, or small talk, with no work to do?"
        }
    })
}

fn p_of(answers: &serde_json::Value, q: &str) -> f64 {
    answers.pointer(&format!("/{q}/noul")).and_then(|v| v.as_f64()).unwrap_or(0.0)
}

/// Laya's reading, only when it is clearly sure: the winning level has at
/// least half the probability and leads the runner-up by a margin. Small talk
/// it recognises is easy whatever the choice said.
pub fn from_laya(answers: &serde_json::Value) -> Option<Difficulty> {
    if p_of(answers, "talk") >= 0.6 {
        return Some(Difficulty::Easy);
    }
    let choice = answers.pointer("/difficulty/choice")?.as_str()?;
    let level = Difficulty::parse(choice)?;
    let probs = answers.pointer("/difficulty/probabilities")?.as_object()?;
    let top = probs.get(choice).and_then(|v| v.as_f64()).unwrap_or(0.0);
    let second = probs.iter().filter(|(k, _)| k.as_str() != choice).filter_map(|(_, v)| v.as_f64()).fold(0.0_f64, f64::max);
    if top < 0.5 || top - second < 0.15 {
        return None;
    }
    // Asked to build something, "easy" is not believed.
    if level == Difficulty::Easy && p_of(answers, "build") >= 0.5 {
        return Some(Difficulty::Medium);
    }
    Some(level)
}

#[derive(Deserialize)]
struct JevAnswer {
    difficulty: String,
    #[serde(default)]
    confidence: f32,
}

/// Decide how hard `text` is. Never fails: the rules always answer.
pub async fn classify(text: &str) -> Verdict {
    // No task in it: nothing for a model to weigh.
    if is_small_talk(text) {
        return Verdict { level: Difficulty::Easy, by: "rules" };
    }
    // The rules are the floor: a model can say a task is harder than the words
    // suggest, but a request to build a website never reads as trivial.
    let floor = rules(text);
    if crate::ai::laya::installed() && crate::ai::laya::enabled().await {
        if let Some(answers) = crate::ai::laya::ask(text, questions(), Duration::from_millis(1500)).await {
            if let Some(level) = from_laya(&answers) {
                return Verdict { level: level.max(floor), by: "laya" };
            }
        }
    }
    if crate::ai::jev::available().await {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "difficulty": { "type": "string", "enum": ["easy", "medium", "hard"], "description": "How hard is this task for an AI agent?" },
                "confidence": { "type": "number", "minimum": 0, "maximum": 1 }
            },
            "required": ["difficulty"]
        });
        if let Some(a) = crate::ai::jev::decide::<JevAnswer>(schema, text).await {
            if a.confidence >= 0.5 {
                if let Some(level) = Difficulty::parse(&a.difficulty) {
                    return Verdict { level: level.max(floor), by: "jev" };
                }
            }
        }
    }
    Verdict { level: floor, by: "rules" }
}

/// Try the classifier on a task, from Settings.
#[tauri::command]
pub async fn cmd_routing_classify(text: String) -> Result<serde_json::Value, String> {
    let v = classify(&text).await;
    Ok(serde_json::json!({
        "level": v.level,
        "by": v.by,
        "laya": crate::ai::laya::installed() && crate::ai::laya::enabled().await,
        "jev": crate::ai::jev::available().await,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_asks_are_easy() {
        assert_eq!(rules("rename the variable foo to bar"), Difficulty::Easy);
        assert_eq!(rules("what is in package.json?"), Difficulty::Easy);
        assert_eq!(rules(""), Difficulty::Easy);
    }

    #[test]
    fn demanding_work_is_hard() {
        let t = "Investigate why the checkout total is wrong across the codebase, find the root cause of the race condition, \
                 then redesign the cart sync so it can't happen again, and plan the migration. Add tests for each path.";
        assert_eq!(rules(t), Difficulty::Hard);
    }

    #[test]
    fn ordinary_tasks_are_in_the_middle() {
        assert_eq!(rules("Add a retry to the upload function and write a test for it, keeping the existing logging style."), Difficulty::Medium);
    }

    #[test]
    fn small_talk_is_easy_without_asking_a_model() {
        for t in ["hi", "Hello!", "hey there", "thanks", "thank you", "ok", "how are you", "good morning"] {
            assert!(is_small_talk(t), "{t}");
            assert_eq!(rules(t), Difficulty::Easy, "{t}");
        }
        assert!(!is_small_talk("create a website"));
        assert!(!is_small_talk("hi, can you fix the login bug"));
    }

    #[test]
    fn building_something_is_never_trivial() {
        assert!(rules("create a website") >= Difficulty::Medium);
        assert!(rules("build me an app") >= Difficulty::Medium);
        assert_eq!(rules("build a full-stack web app with auth, a database and tests"), Difficulty::Hard);
        assert_eq!(rules("make a script that backs up my notes folder"), Difficulty::Medium);
    }

    #[test]
    fn laya_that_calls_a_build_easy_is_not_believed() {
        let a = serde_json::json!({
            "difficulty": { "choice": "easy", "probabilities": { "easy": 0.62, "medium": 0.28, "hard": 0.10 } },
            "build": { "noul": 0.8 },
            "talk": { "noul": 0.02 }
        });
        assert_eq!(from_laya(&a), Some(Difficulty::Medium));
    }

    #[test]
    fn laya_small_talk_is_easy_whatever_the_choice() {
        let a = serde_json::json!({
            "difficulty": { "choice": "medium", "probabilities": { "easy": 0.2, "medium": 0.5, "hard": 0.3 } },
            "build": { "noul": 0.05 },
            "talk": { "noul": 0.9 }
        });
        assert_eq!(from_laya(&a), Some(Difficulty::Easy));
    }

    #[test]
    fn a_narrow_win_is_not_used() {
        let a = serde_json::json!({
            "difficulty": { "choice": "medium", "probabilities": { "easy": 0.4, "medium": 0.5, "hard": 0.1 } },
            "build": { "noul": 0.1 }, "talk": { "noul": 0.1 }
        });
        assert_eq!(from_laya(&a), None);
    }

    #[test]
    fn levels_order_from_easy_to_hard() {
        assert!(Difficulty::Easy < Difficulty::Medium && Difficulty::Medium < Difficulty::Hard);
    }

    #[test]
    fn laya_is_used_only_when_it_is_sure() {
        let sure = serde_json::json!({ "difficulty": { "choice": "hard", "probabilities": { "easy": 0.1, "medium": 0.2, "hard": 0.7 } }, "build": { "noul": 0.9 } });
        assert_eq!(from_laya(&sure), Some(Difficulty::Hard));
        let unsure = serde_json::json!({ "difficulty": { "choice": "medium", "probabilities": { "easy": 0.33, "medium": 0.34, "hard": 0.33 } } });
        assert_eq!(from_laya(&unsure), None);
        assert_eq!(from_laya(&serde_json::json!({})), None);
    }
}

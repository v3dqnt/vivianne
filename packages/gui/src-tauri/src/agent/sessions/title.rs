//! A thread's title: a short summary of what was asked, not a copy of it.
//!
//! Three parts. The pure ones (`fallback_title`, `title_prompt`, `parse_title`,
//! `head_fields`, `decide`) turn text into a title and decide which title a
//! save keeps, and are the ones tested. `on_turn_start` is the glue: when a
//! thread's first message is sent it asks the chat model for a title in the
//! background, never delaying the reply, and keeps the answer until the
//! transcript is saved.
//!
//! A title the user typed (`title_source = "user"`) is never replaced. Before
//! this module an autosave rebuilt the name from the first 30 characters of
//! the first message after every turn, which also threw away a rename.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

use crate::agent::runtime::types::{AgentMessage, MessageContent};

pub const USER: &str = "user";
pub const AUTO: &str = "auto";

/// Longest title, in characters, that a summary or a cleaned sentence may be.
const MAX_CHARS: usize = 48;

/// Writes of a session file by the title task and by autosave don't interleave.
pub static SESSION_WRITE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

// ── Pure ─────────────────────────────────────────────────────────────────

/// What was asked, cleaned into something that reads as a title: the app's own
/// markers, code fences, links and markdown gone, the first sentence only, cut
/// at a word, never in the middle of one.
pub fn fallback_title(text: &str) -> String {
    let mut s = text.to_string();
    // <tags>, ``` fences with their contents, inline code marks, urls, markdown marks.
    while let (Some(a), Some(b)) = (s.find('<'), s.find('>')) {
        if a < b {
            s.replace_range(a..=b, " ");
        } else {
            break;
        }
    }
    while let Some(a) = s.find("```") {
        match s[a + 3..].find("```") {
            Some(b) => s.replace_range(a..a + 3 + b + 3, " "),
            None => {
                s.truncate(a);
                break;
            }
        }
    }
    let words: Vec<&str> = s
        .split_whitespace()
        .filter(|w| !w.starts_with("http://") && !w.starts_with("https://"))
        .collect();
    let mut s = words.join(" ");
    s.retain(|c| !matches!(c, '*' | '_' | '`' | '#' | '>'));
    let s = s.trim();
    // The first sentence: up to a full stop, question or exclamation mark that ends a word.
    let mut end = s.len();
    let chars: Vec<(usize, char)> = s.char_indices().collect();
    for (i, &(at, c)) in chars.iter().enumerate() {
        if matches!(c, '.' | '?' | '!') && chars.get(i + 1).is_none_or(|&(_, n)| n == ' ') && at > 8 {
            end = at + c.len_utf8();
            break;
        }
    }
    let sentence = s[..end].trim().trim_end_matches(['.', ',', ':', ';']);
    if sentence.is_empty() {
        return "New chat".to_string();
    }
    let cut = cut_at_word(sentence, MAX_CHARS);
    let mut out = String::new();
    let mut first = true;
    for c in cut.chars() {
        if first {
            out.extend(c.to_uppercase());
            first = false;
        } else {
            out.push(c);
        }
    }
    out
}

fn cut_at_word(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let head: String = s.chars().take(max).collect();
    let cut = match head.rfind(' ') {
        Some(i) if i >= max / 2 => &head[..i],
        _ => head.as_str(),
    };
    format!("{}…", cut.trim_end_matches([',', ';', ':', '-', ' ']))
}

/// The system and user prompts that ask a model for a title.
pub fn title_prompt(first_ask: &str) -> (String, String) {
    let system = "Write a title for a chat that opens with the message below. Three to six words. Say what the person wants, as a noun phrase or a short imperative (for example: Fix login timeout, Plot cat weights, Review uncommitted changes). Use the same language as the message. No quotes, no trailing full stop, no markdown. Answer with the title only.".to_string();
    let user: String = first_ask.chars().take(1500).collect();
    (system, user)
}

/// A model's answer as a title, or `None` when it isn't one.
pub fn parse_title(answer: &str) -> Option<String> {
    let line = answer.lines().map(str::trim).find(|l| !l.is_empty())?;
    let mut t = line.trim();
    for prefix in ["Title:", "title:", "TITLE:"] {
        if let Some(rest) = t.strip_prefix(prefix) {
            t = rest.trim();
        }
    }
    let t = t.trim_matches(|c: char| matches!(c, '"' | '\'' | '`' | '*' | '“' | '”' | '‘' | '’' | '#')).trim().trim_end_matches('.').trim();
    if t.is_empty() || t.chars().count() > 60 || t.split_whitespace().count() > 12 {
        return None;
    }
    Some(t.to_string())
}

/// `name` and `title_source` from the start of a session file, without
/// reading the transcript that follows (it can be tens of megabytes). Both
/// fields are written before `messages`.
pub fn head_fields(head: &str) -> (Option<String>, Option<String>) {
    fn field(head: &str, key: &str) -> Option<String> {
        let at = head.find(&format!("\"{key}\""))?;
        let rest = &head[at + key.len() + 2..];
        let rest = rest.trim_start().strip_prefix(':')?.trim_start().strip_prefix('"')?;
        let mut out = String::new();
        let mut chars = rest.chars();
        while let Some(c) = chars.next() {
            match c {
                '"' => return Some(out),
                '\\' => match chars.next()? {
                    'n' => out.push('\n'),
                    't' => out.push('\t'),
                    'u' => {
                        let hex: String = chars.by_ref().take(4).collect();
                        out.push(u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32)?);
                    }
                    other => out.push(other),
                },
                c => out.push(c),
            }
        }
        None
    }
    (field(head, "name"), field(head, "title_source"))
}

/// The title a save writes, and its source.
///
/// - a title the user typed stays;
/// - a title already made (by the model or from a sentence) stays;
/// - a session from before titles had a source is named from the first
///   message, and is then "auto", which a later rename turns into "user".
pub fn decide(existing_name: Option<&str>, existing_source: Option<&str>, first_ask: Option<&str>) -> (String, &'static str) {
    match (existing_source, existing_name) {
        (Some(USER), Some(n)) if !n.trim().is_empty() => (n.to_string(), USER),
        (Some(AUTO), Some(n)) if !n.trim().is_empty() && n != "New chat" && n != "Untitled Session" => (n.to_string(), AUTO),
        _ => (first_ask.map(fallback_title).unwrap_or_else(|| "New chat".to_string()), AUTO),
    }
}

/// The first thing a person asked: the text of the first user message that
/// isn't the handoff note a day opens with.
pub fn first_ask(messages: &[AgentMessage]) -> Option<String> {
    messages.iter().find_map(|m| match m {
        AgentMessage::User { content, .. } if !crate::agent::sessions::days::is_handoff(m) => content.iter().find_map(|c| match c {
            // A compaction summary is carried as a user message; it is not what anyone asked.
            MessageContent::Text { text } if !text.trim().is_empty() && !text.trim_start().starts_with("<compacted_history>") => Some(text.clone()),
            _ => None,
        }),
        _ => None,
    })
}

// ── The cache between "a title was made" and "the transcript is saved" ───

static TITLES: LazyLock<Mutex<HashMap<String, (String, &'static str)>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

pub fn remember(id: &str, name: &str, source: &'static str) {
    TITLES.lock().unwrap_or_else(|e| e.into_inner()).insert(id.to_string(), (name.to_string(), source));
}

pub fn remembered(id: &str) -> Option<(String, &'static str)> {
    TITLES.lock().unwrap_or_else(|e| e.into_inner()).get(id).cloned()
}

pub fn forget(id: &str) {
    TITLES.lock().unwrap_or_else(|e| e.into_inner()).remove(id);
}

/// The model titles are written with: the one the chat uses. A coding agent
/// can't answer a one-shot question, so a thread on one borrows the last
/// ordinary model that was picked.
static LAST_CHAT_MODEL: LazyLock<Mutex<Option<crate::agent::runtime::types::ModelRef>>> = LazyLock::new(|| Mutex::new(None));

pub fn note_chat_model(model: &crate::agent::runtime::types::ModelRef) {
    if model.provider != crate::agent::harness::PROVIDER && model.provider != crate::ai::imagegen::LOCAL && !model.id.is_empty() {
        *LAST_CHAT_MODEL.lock().unwrap_or_else(|e| e.into_inner()) = Some(model.clone());
    }
}

/// Called when a turn of the main chat starts. If it is the thread's first
/// message and nothing has titled it, a title is asked for in the background.
pub async fn on_turn_start(app: &tauri::AppHandle, state: &crate::agent::runtime::commands::AppState, text: &str) {
    use tauri::Emitter;
    let Some(id) = state.session_id().await else { return };
    let model = state.agent_state.model().await;
    note_chat_model(&model);
    if remembered(&id).is_some() {
        return;
    }
    let before = state.agent_state.messages().await;
    if first_ask(&before).is_some() {
        return; // not the first message
    }
    // Until the model answers, the cleaned sentence is the title.
    let fallback = fallback_title(text);
    remember(&id, &fallback, AUTO);
    let Some(chat_model) = (if model.provider == crate::agent::harness::PROVIDER { LAST_CHAT_MODEL.lock().unwrap_or_else(|e| e.into_inner()).clone() } else { Some(model) }) else { return };
    let app = app.clone();
    let text = text.to_string();
    tokio::spawn(async move {
        let chat_model = if crate::agent::models::routing::is_router(&chat_model) {
            match crate::agent::models::routing::resolve_router(&text).await {
                Ok((m, _)) => m,
                Err(_) => return,
            }
        } else {
            chat_model
        };
        let (system, user) = title_prompt(&text);
        let answer = tokio::time::timeout(std::time::Duration::from_secs(15), crate::ai::completion::complete_background(&app, &chat_model, &system, &user)).await;
        let title = match &answer {
            Ok(Ok(text)) => parse_title(text),
            Ok(Err(e)) => {
                log::warn!("[title] {} couldn't write a title: {e}", chat_model.id);
                None
            }
            Err(_) => {
                log::warn!("[title] {} took over 15s to write a title; keeping the sentence", chat_model.id);
                None
            }
        };
        let Some(title) = title else {
            if let Ok(Ok(text)) = &answer {
                log::warn!("[title] not a title: {:?}", text.chars().take(120).collect::<String>());
            }
            return;
        };
        log::info!("[title] {id}: {title}");
        // A rename while the model was thinking wins.
        if remembered(&id).is_some_and(|(_, src)| src == USER) {
            return;
        }
        remember(&id, &title, AUTO);
        if crate::agent::runtime::commands::rename_session_file(&id, &title, AUTO).await.is_ok() {
            let _ = app.emit("session:renamed", serde_json::json!({ "id": id, "name": title }));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_first_message_becomes_a_clean_short_title() {
        assert_eq!(fallback_title("can you create a tool to launch the app from the tray? i also want it to start on boot"), "Can you create a tool to launch the app from…");
        assert_eq!(fallback_title("fix the login timeout"), "Fix the login timeout");
        assert_eq!(fallback_title("Reply with exactly the single word: ok"), "Reply with exactly the single word: ok");
    }

    #[test]
    fn markers_code_links_and_markdown_do_not_reach_a_title() {
        assert_eq!(fallback_title("<system-reminder>x</system-reminder> **Review** my `uncommitted` changes"), "X Review my uncommitted changes");
        assert_eq!(fallback_title("look at https://example.com/very/long/path and tell me why"), "Look at and tell me why");
        assert_eq!(fallback_title("```rust\nfn main() {}\n``` why does this not compile?"), "Why does this not compile?");
        assert_eq!(fallback_title("```\nunterminated"), "New chat");
        assert_eq!(fallback_title("   \n  "), "New chat");
    }

    #[test]
    fn a_long_title_is_cut_at_a_word_never_inside_one() {
        let t = fallback_title("research the history of the transatlantic telegraph cable and its financing");
        assert!(t.ends_with('…') && t.chars().count() <= MAX_CHARS + 1, "{t}");
        assert!(!t.trim_end_matches('…').ends_with("telegra"), "{t}");
        let one_word = fallback_title(&"x".repeat(200));
        assert!(one_word.chars().count() <= MAX_CHARS + 1);
        assert_eq!(fallback_title("héllo wörld, ça va très bien aujourd'hui, merci beaucoup de demander").chars().count() <= MAX_CHARS + 1, true);
    }

    #[test]
    fn a_models_answer_becomes_a_title_or_nothing() {
        assert_eq!(parse_title("Fix login timeout").as_deref(), Some("Fix login timeout"));
        assert_eq!(parse_title("\"Plot cat weights.\"").as_deref(), Some("Plot cat weights"));
        assert_eq!(parse_title("Title: **Review changes**\nBecause the user asked").as_deref(), Some("Review changes"));
        assert_eq!(parse_title("\n\n  Sort a list  \n").as_deref(), Some("Sort a list"));
        assert_eq!(parse_title(""), None);
        assert_eq!(parse_title("   "), None);
        assert_eq!(parse_title(&"word ".repeat(30)), None, "a paragraph is not a title");
    }

    #[test]
    fn the_prompt_asks_for_a_short_title_in_the_messages_language() {
        let (system, user) = title_prompt("hola, ¿puedes ayudarme?");
        assert!(system.contains("Three to six words") && system.contains("same language"));
        assert_eq!(user, "hola, ¿puedes ayudarme?");
        assert_eq!(title_prompt(&"a".repeat(5000)).1.chars().count(), 1500);
    }

    #[test]
    fn the_name_is_read_from_the_head_of_a_session_file() {
        let head = "{\n  \"id\": \"session_1\",\n  \"name\": \"Fix \\\"login\\\" timeout\",\n  \"title_source\": \"user\",\n  \"messages\": [";
        assert_eq!(head_fields(head), (Some("Fix \"login\" timeout".to_string()), Some("user".to_string())));
        assert_eq!(head_fields("{\"id\":\"s\",\"name\":\"Old\",\"messages\":[]}"), (Some("Old".to_string()), None));
        assert_eq!(head_fields("{\"id\":\"s\",\"name\":\"caf\\u00e9\"}").0.as_deref(), Some("café"));
        assert_eq!(head_fields("garbage"), (None, None));
    }

    #[test]
    fn a_rename_sticks_a_made_title_stays_and_an_old_name_is_redone() {
        assert_eq!(decide(Some("My name"), Some(USER), Some("whatever")), ("My name".to_string(), USER));
        assert_eq!(decide(Some("Fix login timeout"), Some(AUTO), Some("fix the login")), ("Fix login timeout".to_string(), AUTO));
        // The placeholder a new chat is written with is not a title.
        assert_eq!(decide(Some("New chat"), Some(AUTO), Some("plot cat weights")), ("Plot cat weights".to_string(), AUTO));
        // From before titles had a source: redo it from the first message.
        assert_eq!(decide(Some("can you create a tool to launc"), None, Some("can you create a tool to launch it")), ("Can you create a tool to launch it".to_string(), AUTO));
        assert_eq!(decide(None, None, None), ("New chat".to_string(), AUTO));
    }

    #[test]
    fn a_compaction_summary_is_not_the_first_thing_anyone_asked() {
        let user = |t: &str| AgentMessage::User { content: vec![MessageContent::Text { text: t.into() }], timestamp: 0 };
        let msgs = vec![user("<compacted_history>
**Goal** old work"), user("now fix the tests")];
        assert_eq!(first_ask(&msgs).as_deref(), Some("now fix the tests"));
        assert_eq!(first_ask(&msgs[..1]), None);
    }

    #[test]
    fn a_title_waits_in_the_cache_until_it_is_saved() {
        remember("t-1", "Plot cat weights", AUTO);
        assert_eq!(remembered("t-1"), Some(("Plot cat weights".to_string(), AUTO)));
        remember("t-1", "Mine", USER);
        assert_eq!(remembered("t-1").unwrap().1, USER);
        forget("t-1");
        assert_eq!(remembered("t-1"), None);
    }
}

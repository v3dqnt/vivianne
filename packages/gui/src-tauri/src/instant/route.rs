//! What a spoken request is, before anything acts on it.
//!
//! The notch used to treat every utterance as something to do on the computer.
//! Now it decides first, between four things it handles differently:
//!
//! - **computer** — "open Spotify", "close these windows": drive the desktop
//!   with `dip`, as before.
//! - **general** — "what's the capital of Australia": answer it, out loud.
//! - **project** — anything about one of the user's projects. A question is
//!   answered from that project's files; a request for work ("add a dark mode
//!   toggle") is handed to the project's own agent in the app.
//! - **status** — "what are my agents doing": answered from the notch's own
//!   status, no model needed.
//!
//! [`laya`](crate::ai::laya) makes the decision when it's installed. Its
//! answers are combined with what can be read off the words directly — a
//! project named outright is that project, whatever the classifier says — and
//! [`rules`] alone decides when it isn't there.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Computer,
    General,
    Project,
    Status,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectRef {
    pub id: String,
    pub name: String,
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Route {
    pub kind: Kind,
    pub project: Option<ProjectRef>,
    /// Asks for something to be built, changed or done, not for information.
    pub work: bool,
    /// "laya" or "rules": what decided.
    pub by: &'static str,
}

/// Lowercase, with anything that isn't a letter or digit turned into a single
/// space, so "agent-coreutils" and "Agent Coreutils" match.
fn norm(s: &str) -> String {
    let mapped: String = s.chars().map(|c| if c.is_alphanumeric() { c.to_ascii_lowercase() } else { ' ' }).collect();
    mapped.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A project named outright in what was said.
fn named_project<'a>(text: &str, projects: &'a [ProjectRef]) -> Option<&'a ProjectRef> {
    let said = format!(" {} ", norm(text));
    let squashed = said.replace(' ', "");
    projects
        .iter()
        .filter(|p| {
            let name = norm(&p.name);
            // Too short to tell apart from ordinary words ("ai", "go").
            name.len() >= 3 && (said.contains(&format!(" {name} ")) || (name.contains(' ') && squashed.contains(&name.replace(' ', ""))))
        })
        // The longest name wins: "vivianne guides" over "vivianne".
        .max_by_key(|p| p.name.len())
}

fn starts_with_any(text: &str, words: &[&str]) -> bool {
    words.iter().any(|w| text == *w || text.starts_with(&format!("{w} ")))
}

const COMPUTER_VERBS: &[&str] = &[
    "open", "launch", "start", "close", "quit", "click", "double click", "type", "press", "scroll", "switch to", "go to",
    "minimize", "minimise", "maximize", "maximise", "play", "pause", "mute", "unmute", "turn up", "turn down", "show me",
    "point at", "where is", "find the", "select", "copy", "paste", "save", "take a screenshot", "move", "drag", "focus",
];
const WORK_VERBS: &[&str] = &[
    "add", "build", "make", "create", "fix", "implement", "change", "update", "refactor", "remove", "delete", "write",
    "rename", "ship", "deploy", "test", "run", "set up", "setup", "migrate", "improve", "optimise", "optimize", "finish",
];
const STATUS_HINTS: &[&str] = &[
    "my agents", "the agents", "agents doing", "agent doing", "what's running", "what is running", "how far", "progress",
    "status", "still working", "are you done", "is it done", "what are you doing", "what are you working",
];

fn is_work(t: &str) -> bool {
    WORK_VERBS.iter().any(|w| t.starts_with(&format!("{w} ")) || t.contains(&format!(" {w} ")) || t.contains(&format!("please {w} ")))
}

/// The decision from the words alone.
pub fn rules(text: &str, projects: &[ProjectRef]) -> Route {
    let t = norm(text);
    let mk = |kind, project: Option<&ProjectRef>| Route { kind, project: project.cloned(), work: is_work(&t), by: "rules" };
    if let Some(p) = named_project(text, projects) {
        return mk(Kind::Project, Some(p));
    }
    if STATUS_HINTS.iter().any(|h| t.contains(&norm(h))) {
        return mk(Kind::Status, None);
    }
    if starts_with_any(&t, COMPUTER_VERBS) || starts_with_any(t.trim_start_matches("please "), COMPUTER_VERBS) {
        return mk(Kind::Computer, None);
    }
    let question = text.trim_end().ends_with('?')
        || starts_with_any(&t, &["what", "who", "why", "when", "where", "how", "is", "are", "can", "could", "should", "do", "does", "tell me", "explain", "define"]);
    if question {
        return mk(Kind::General, None);
    }
    // An imperative that isn't about a project: the notch's original job.
    mk(Kind::Computer, None)
}

/// The questions put to Laya. Project labels are their ids, described by name
/// and what the project is about.
pub fn questions(projects: &[(ProjectRef, String)]) -> serde_json::Value {
    let mut criteria = serde_json::Map::new();
    for (p, about) in projects {
        criteria.insert(p.id.clone(), serde_json::Value::String(about.clone()));
    }
    criteria.insert("none".into(), "none of these projects".into());
    serde_json::json!({
        "kind": {
            "type": "choice",
            "instructions": "What is the user asking the voice assistant to do?",
            "criteria": {
                "computer": "operate apps on this computer right now: open, click, type, switch or close windows, play, scroll",
                "project": "about one of the user's software projects, its code, features, bugs or progress",
                "status": "what the assistant's agents are doing, task progress or what is running",
                "general": "a general question, fact, advice, calculation, or conversation"
            }
        },
        "project": {
            "type": "choice",
            "instructions": "Which of the user's projects is this about?",
            "criteria": criteria
        },
        "work": {
            "type": "noul",
            "instructions": "Does the user ask for something to be built, changed, fixed or done, rather than for information?"
        }
    })
}

fn prob(a: &serde_json::Value, q: &str, label: &str) -> f64 {
    a.pointer(&format!("/{q}/probabilities/{label}")).and_then(|v| v.as_f64()).unwrap_or(0.0)
}

/// Laya's answers, combined with what the words say outright.
pub fn combine(answers: &serde_json::Value, text: &str, projects: &[ProjectRef]) -> Route {
    let choice = |q: &str| answers.pointer(&format!("/{q}/choice")).and_then(|v| v.as_str()).unwrap_or("");
    let work = answers.pointer("/work/noul").and_then(|v| v.as_f64()).unwrap_or(0.0) >= 0.5;
    let picked = projects.iter().find(|p| p.id == choice("project"));
    let named = named_project(text, projects);
    let project_conf = picked.map(|p| prob(answers, "project", &p.id)).unwrap_or(0.0);
    let mut kind = match choice("kind") {
        "computer" => Kind::Computer,
        "project" => Kind::Project,
        "status" => Kind::Status,
        _ => Kind::General,
    };
    // A project named outright, or picked with confidence, outranks a
    // close call between the other kinds: "fix the failing tests in agent
    // coreutils" is about that project even when "status" edges ahead. Only a
    // confident "do this on the computer" keeps it.
    let computer_sure = kind == Kind::Computer && prob(answers, "kind", "computer") >= 0.5;
    if (named.is_some() || project_conf >= 0.5) && !computer_sure {
        kind = Kind::Project;
    }
    let project = if kind == Kind::Project { named.or(picked).cloned() } else { None };
    // "Project" with nothing to point at is a question about the user's work
    // in general; it is answered like any other question.
    if kind == Kind::Project && project.is_none() {
        kind = Kind::General;
    }
    Route { kind, project, work, by: "laya" }
}

/// A line about what a project is, for Laya's choice: its name, and the
/// first lines of its charter when it has one.
fn describe(p: &ProjectRef) -> String {
    let charter = p
        .path
        .as_deref()
        .and_then(|root| std::fs::read_to_string(std::path::Path::new(root).join(".viv").join("charter.md")).ok())
        .map(|c| c.lines().filter(|l| !l.trim().is_empty() && !l.starts_with('#')).take(2).collect::<Vec<_>>().join(" "))
        .unwrap_or_default();
    let folder = p.path.as_deref().and_then(|r| std::path::Path::new(r).file_name()).map(|f| f.to_string_lossy().to_string()).unwrap_or_default();
    let mut about = format!("the {} project", p.name);
    if !folder.is_empty() && norm(&folder) != norm(&p.name) {
        about.push_str(&format!(" (folder {folder})"));
    }
    if !charter.is_empty() {
        about.push_str(": ");
        about.extend(charter.chars().take(220));
    }
    about
}

pub async fn projects() -> Vec<ProjectRef> {
    crate::agent::projects::cmd_list_projects()
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|p| ProjectRef { id: p.id, name: p.name, path: p.path })
        .collect()
}

/// Decide what `text` is.
pub async fn route(text: &str) -> Route {
    let projects = projects().await;
    let described: Vec<_> = projects.iter().map(|p| (p.clone(), describe(p))).collect();
    match crate::ai::laya::ask(text, questions(&described), std::time::Duration::from_secs(4)).await {
        Some(answers) => combine(&answers, text, &projects),
        None => rules(text, &projects),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn projects() -> Vec<ProjectRef> {
        ["Vivianne", "Trailer", "agent-coreutils"]
            .iter()
            .enumerate()
            .map(|(i, n)| ProjectRef { id: format!("p{i}"), name: n.to_string(), path: None })
            .collect()
    }

    #[test]
    fn rules_tell_the_kinds_apart() {
        let ps = projects();
        let k = |t: &str| rules(t, &ps).kind;
        assert_eq!(k("open spotify and play my liked songs"), Kind::Computer);
        assert_eq!(k("Close all the Chrome windows"), Kind::Computer);
        assert_eq!(k("what's the capital of australia?"), Kind::General);
        assert_eq!(k("tell me a joke"), Kind::General);
        assert_eq!(k("what are my agents doing right now"), Kind::Status);
        assert_eq!(k("in Vivianne, add a dark mode toggle"), Kind::Project);
        assert_eq!(k("fix the failing tests in agent coreutils"), Kind::Project);
        assert_eq!(rules("fix the failing tests in agent coreutils", &ps).project.unwrap().id, "p2");
        assert!(rules("in vivianne add a dark mode toggle", &ps).work);
        assert!(!rules("what does the notch do in vivianne", &ps).work);
    }

    #[test]
    fn a_named_project_outranks_a_close_call() {
        let ps = projects();
        // Laya's real answer for this sentence: "status" by a hair.
        let answers = serde_json::json!({
            "kind": { "choice": "status", "probabilities": { "computer": 0.14, "project": 0.37, "status": 0.38, "general": 0.11 } },
            "project": { "choice": "p2", "probabilities": { "p0": 0.13, "p1": 0.03, "p2": 0.59, "none": 0.26 } },
            "work": { "noul": 0.81 }
        });
        let r = combine(&answers, "fix the failing tests in agent coreutils", &ps);
        assert_eq!(r.kind, Kind::Project);
        assert_eq!(r.project.unwrap().id, "p2");
        assert!(r.work);
    }

    #[test]
    fn a_project_by_name_even_when_laya_misses_it() {
        let ps = projects();
        let answers = serde_json::json!({
            "kind": { "choice": "project", "probabilities": { "project": 0.6 } },
            "project": { "choice": "none", "probabilities": { "none": 0.7 } },
            "work": { "noul": 0.14 }
        });
        let r = combine(&answers, "what's left on the trailer roadmap", &ps);
        assert_eq!(r.project.unwrap().id, "p1");
        assert!(!r.work);
    }

    #[test]
    fn a_confident_computer_action_stays_one() {
        let ps = projects();
        let answers = serde_json::json!({
            "kind": { "choice": "computer", "probabilities": { "computer": 0.8 } },
            "project": { "choice": "p0", "probabilities": { "p0": 0.55 } },
            "work": { "noul": 0.8 }
        });
        assert_eq!(combine(&answers, "open the vivianne folder in explorer", &ps).kind, Kind::Computer);
    }
}

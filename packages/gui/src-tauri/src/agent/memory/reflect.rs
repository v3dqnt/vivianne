//! `reflect`: answer a question from memory by looking in the right order and saying where the answer came from.
//!
//! The idea is Hindsight's reflect (vectorize-io/hindsight, MIT): an agent loop that searches the project's pages first, then its
//! observations, then raw facts, and answers with citations. Differences here: the loop is bounded (`MAX_STEPS`), a citation counts only if
//! it names something the loop actually retrieved, and the loop has no disposition or persona of its own: the answer is for the agent that
//! asked, and Vivianne's own rules (the user's words outrank an inference, see `evolve::provenance_label`) are shown with every hit.

use futures::future::BoxFuture;
use serde_json::Value;

pub const MAX_STEPS: usize = 6;
const HITS_PER_STEP: usize = 6;
const SHOWN_CHARS: usize = 400;
const SOURCE_CHARS: usize = 140;

#[derive(Debug, Clone, PartialEq)]
pub struct Hit {
    pub id: String,
    pub label: String,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Pages,
    Observations,
    Facts,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    Search { target: Target, query: String },
    Expand { id: String },
    Answer { text: String, cites: Vec<String> },
}

/// The model's next step, read from its reply (one JSON object, possibly in a fence or with prose around it). `None` when it is not one.
pub fn parse_step(reply: &str) -> Option<Step> {
    let start = reply.find('{')?;
    let end = reply.rfind('}')?;
    let v: Value = serde_json::from_str(reply.get(start..=end)?).ok()?;
    let text = |k: &str| v.get(k).and_then(|x| x.as_str()).map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    match v.get("action")?.as_str()?.to_lowercase().as_str() {
        "search_pages" => Some(Step::Search { target: Target::Pages, query: text("query")? }),
        "search_observations" => Some(Step::Search { target: Target::Observations, query: text("query")? }),
        "recall" | "search_facts" => Some(Step::Search { target: Target::Facts, query: text("query")? }),
        "expand" => Some(Step::Expand { id: text("id")? }),
        "answer" => {
            let cites = v.get("cites").and_then(|c| c.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(|s| s.trim().to_string())).collect()).unwrap_or_default();
            Some(Step::Answer { text: text("text")?, cites })
        }
        _ => None,
    }
}

pub const SYSTEM_PROMPT: &str = "You answer a question from an agent's long-term memory. Look in this order: the project's PAGES (standing answers), then OBSERVATIONS (beliefs drawn from several facts), then raw FACTS. \
Each turn reply with ONE JSON object and nothing else: \
{\"action\": \"search_pages\", \"query\": \"...\"} | {\"action\": \"search_observations\", \"query\": \"...\"} | {\"action\": \"recall\", \"query\": \"...\"} | {\"action\": \"expand\", \"id\": \"<id of a hit>\"} (shows what an observation rests on) | \
{\"action\": \"answer\", \"text\": \"...\", \"cites\": [\"<ids of the hits the answer rests on>\"]}. \
Answer as soon as what you have is enough; at most six steps. Say only what the hits support, and say so plainly when memory does not have it. \
Each hit is labelled by where it came from: what the user saved on purpose or told you outranks what was inferred. Cite only ids you were shown.";

fn short(id: &str) -> &str {
    id.get(..8).unwrap_or(id)
}

fn clip(text: &str, max: usize) -> String {
    let t = text.replace('\n', " ");
    if t.chars().count() <= max {
        return t;
    }
    format!("{}…", t.chars().take(max).collect::<String>())
}

fn show(hits: &[Hit]) -> String {
    if hits.is_empty() {
        return "(nothing found)".to_string();
    }
    hits.iter().map(|h| format!("- [{}] ({}) {}", short(&h.id), h.label, clip(&h.text, SHOWN_CHARS))).collect::<Vec<_>>().join("\n")
}

#[derive(Debug, Clone, PartialEq)]
pub struct Outcome {
    pub answer: String,
    /// What the answer rests on: the cited hits, or when the model cited nothing valid, none.
    pub sources: Vec<Hit>,
    pub steps: usize,
    /// False when the step budget ran out and the answer was forced.
    pub finished: bool,
}

/// Resolve a cited id (full, or the short form that was shown) to a hit that was retrieved.
fn resolve<'a>(cite: &str, seen: &'a [Hit]) -> Option<&'a Hit> {
    let cite = cite.trim().trim_matches(['[', ']']);
    if cite.len() < 4 {
        return None;
    }
    seen.iter().find(|h| h.id == cite || short(&h.id) == cite)
}

/// Run the loop. `ask` is one model call; `fetch` runs a search or an expansion and returns what it found.
pub async fn run(
    question: &str,
    ask: &(dyn Fn(String) -> BoxFuture<'static, Result<String, String>> + Sync),
    fetch: &(dyn Fn(Step) -> BoxFuture<'static, Vec<Hit>> + Sync),
) -> Result<Outcome, String> {
    let mut seen: Vec<Hit> = Vec::new();
    let mut log = String::new();
    for step_no in 1..=MAX_STEPS + 1 {
        let forced = step_no > MAX_STEPS;
        let mut prompt = format!("QUESTION: {question}\n\n");
        if !log.is_empty() {
            prompt.push_str(&format!("SO FAR:\n{log}\n"));
        }
        if forced {
            prompt.push_str("\nYou are out of steps: answer now with what you have.");
        }
        let reply = ask(prompt).await?;
        let Some(step) = parse_step(&reply) else {
            // Not a step: if it is plain prose after something was found, take it as the answer rather than spending another call.
            if !seen.is_empty() && !reply.trim().is_empty() && !forced {
                log.push_str("(your last reply was not a JSON object; reply with one)\n");
                continue;
            }
            return Ok(Outcome { answer: reply.trim().to_string(), sources: Vec::new(), steps: step_no, finished: false });
        };
        match step {
            Step::Answer { text, cites } => {
                let mut sources: Vec<Hit> = Vec::new();
                for c in &cites {
                    if let Some(h) = resolve(c, &seen) {
                        if !sources.iter().any(|s| s.id == h.id) {
                            sources.push(h.clone());
                        }
                    }
                }
                return Ok(Outcome { answer: text, sources, steps: step_no, finished: !forced });
            }
            _ if forced => continue,
            step => {
                let what = match &step {
                    Step::Search { target, query } => format!("{} for \"{query}\"", match target { Target::Pages => "search_pages", Target::Observations => "search_observations", Target::Facts => "recall" }),
                    Step::Expand { id } => format!("expand {id}"),
                    Step::Answer { .. } => String::new(),
                };
                let step = match step {
                    // An expansion names a hit by the short id it was shown under.
                    Step::Expand { id } => match resolve(&id, &seen) {
                        Some(h) => Step::Expand { id: h.id.clone() },
                        None => {
                            log.push_str(&format!("> {what}\n(no such hit: expand an id you were shown)\n\n"));
                            continue;
                        }
                    },
                    other => other,
                };
                let hits: Vec<Hit> = fetch(step).await.into_iter().take(HITS_PER_STEP).collect();
                log.push_str(&format!("> {what}\n{}\n\n", show(&hits)));
                for h in hits {
                    if !seen.iter().any(|s| s.id == h.id) {
                        seen.push(h);
                    }
                }
            }
        }
    }
    Ok(Outcome { answer: "Could not reach an answer from memory.".to_string(), sources: Vec::new(), steps: MAX_STEPS, finished: false })
}

/// What the tool returns: the answer and where it came from.
pub fn render(o: &Outcome) -> String {
    let mut out = o.answer.trim().to_string();
    if !o.sources.is_empty() {
        out.push_str("\n\nSources:");
        for s in &o.sources {
            out.push_str(&format!("\n- [{}] ({}) {}", short(&s.id), s.label, clip(&s.text, SOURCE_CHARS)));
        }
    } else if o.finished {
        out.push_str("\n\n(No memory was cited for this.)");
    }
    if !o.finished {
        out.push_str("\n(Stopped before the model finished; treat this as partial.)");
    }
    out
}

async fn labelled(entries: Vec<super::MemoryEntry>) -> Vec<Hit> {
    entries.into_iter().map(|e| Hit { label: super::evolve::provenance_label(&e.kind).to_string(), id: e.id, text: e.text }).collect()
}

/// Search one kind of memory in `root`: keyword and meaning, then only that kind.
async fn search_kind(root: &str, query: &str, kind: &str, auth: &std::sync::Arc<crate::ai::auth::AuthState>) -> Vec<Hit> {
    let mut found = super::store::search_fts(root, query, 40).await.unwrap_or_default();
    if let Ok(Some(v)) = crate::ai::embedding::embed_query(Some(auth.as_ref()), query).await {
        for (e, _) in super::store::search_vector_scored(root, &v, 40).await.unwrap_or_default() {
            if !found.iter().any(|f| f.id == e.id) {
                found.push(e);
            }
        }
    }
    found.retain(|e| e.kind == kind);
    labelled(found).await
}

/// What an observation rests on, or an entry's own text for anything else.
async fn expand(root: &str, id: &str) -> Vec<Hit> {
    let own = super::store::get_entries_by_ids(root, &[id.to_string()]).await.unwrap_or_default();
    let Some(entry) = own.into_iter().next() else { return Vec::new() };
    let mut out = labelled(vec![entry.clone()]).await;
    if entry.kind == "observation" {
        let (root_c, id_c) = (root.to_string(), id.to_string());
        let ids: Vec<String> = tokio::task::spawn_blocking(move || {
            super::store::with_conn(&root_c, |c| {
                let mut stmt = c.prepare("SELECT entry_id FROM observation_evidence WHERE observation_id = ?1 LIMIT 12").map_err(|e| e.to_string())?;
                let rows = stmt.query_map([&id_c], |r| r.get::<_, String>(0)).map_err(|e| e.to_string())?;
                Ok(rows.flatten().collect())
            })
        })
        .await
        .ok()
        .and_then(Result::ok)
        .unwrap_or_default();
        out.extend(labelled(super::store::get_entries_by_ids(root, &ids).await.unwrap_or_default()).await);
    }
    out
}

/// The tool: answer `question` from `root`'s memory.
pub async fn answer(app: &tauri::AppHandle, root: &str, model: &crate::agent::runtime::types::ModelRef, question: &str) -> Result<String, String> {
    if !super::state_snapshot(root).await.enabled {
        return Ok("Memory is off for this project, so there is nothing to reflect on.".to_string());
    }
    let auth = crate::ai::auth::wait_for_state(app).await;
    let (app, model_c, root_c) = (app.clone(), model.clone(), root.to_string());
    let ask = move |prompt: String| -> BoxFuture<'static, Result<String, String>> {
        let (app, model) = (app.clone(), model_c.clone());
        Box::pin(async move { crate::ai::completion::complete_background(&app, &model, SYSTEM_PROMPT, &prompt).await })
    };
    let auth_c = auth.clone();
    let fetch = move |step: Step| -> BoxFuture<'static, Vec<Hit>> {
        let (root, auth) = (root_c.clone(), auth_c.clone());
        Box::pin(async move {
            match step {
                Step::Search { target: Target::Pages, query } => search_kind(&root, &query, "page", &auth).await,
                Step::Search { target: Target::Observations, query } => search_kind(&root, &query, "observation", &auth).await,
                Step::Search { target: Target::Facts, query } => labelled(super::recall_at(&root, &query, None, Some(&auth), None).await).await,
                Step::Expand { id } => expand(&root, &id).await,
                Step::Answer { .. } => Vec::new(),
            }
        })
    };
    run(question, &ask, &fetch).await.map(|o| render(&o))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    fn hit(id: &str, label: &str, text: &str) -> Hit {
        Hit { id: id.into(), label: label.into(), text: text.into() }
    }

    /// A scripted model and a scripted memory.
    fn scripted(replies: Vec<&'static str>, memory: Vec<(Step, Vec<Hit>)>) -> (impl Fn(String) -> BoxFuture<'static, Result<String, String>> + Sync, impl Fn(Step) -> BoxFuture<'static, Vec<Hit>> + Sync, std::sync::Arc<Mutex<Vec<String>>>) {
        let prompts = std::sync::Arc::new(Mutex::new(Vec::new()));
        let (p2, replies) = (prompts.clone(), Mutex::new(replies.into_iter()));
        let ask = move |prompt: String| -> BoxFuture<'static, Result<String, String>> {
            p2.lock().unwrap().push(prompt);
            let next = replies.lock().unwrap().next().map(String::from).ok_or_else(|| "script ran out".to_string());
            Box::pin(async move { next })
        };
        let fetch = move |step: Step| -> BoxFuture<'static, Vec<Hit>> {
            let found = memory.iter().find(|(s, _)| *s == step).map(|(_, h)| h.clone()).unwrap_or_default();
            Box::pin(async move { found })
        };
        (ask, fetch, prompts)
    }

    #[test]
    fn steps_are_read_from_a_reply_with_or_without_a_fence() {
        assert_eq!(parse_step("{\"action\": \"search_pages\", \"query\": \"releases\"}"), Some(Step::Search { target: Target::Pages, query: "releases".into() }));
        assert_eq!(parse_step("```json\n{\"action\":\"recall\",\"query\":\"vite port\"}\n```"), Some(Step::Search { target: Target::Facts, query: "vite port".into() }));
        assert_eq!(parse_step("Let me look. {\"action\": \"expand\", \"id\": \"abcd1234\"} ok"), Some(Step::Expand { id: "abcd1234".into() }));
        assert_eq!(parse_step("{\"action\": \"answer\", \"text\": \"Tag from main\", \"cites\": [\"a1\", 5, \"b2\"]}"), Some(Step::Answer { text: "Tag from main".into(), cites: vec!["a1".into(), "b2".into()] }));
        for bad in ["", "no json", "{\"action\": \"search_pages\"}", "{\"action\": \"delete\", \"id\": \"x\"}", "{\"query\": \"x\"}", "{\"action\": \"answer\", \"text\": \"  \"}"] {
            assert_eq!(parse_step(bad), None, "{bad}");
        }
    }

    #[tokio::test]
    async fn it_looks_pages_first_and_answers_citing_what_it_found() {
        let page = hit("aaaaaaaa-1111", "project page, kept up to date", "Releases\n- Tag from main");
        let (ask, fetch, prompts) = scripted(
            vec![
                "{\"action\": \"search_pages\", \"query\": \"how do we release\"}",
                "{\"action\": \"answer\", \"text\": \"Tag from main.\", \"cites\": [\"aaaaaaaa\"]}",
            ],
            vec![(Step::Search { target: Target::Pages, query: "how do we release".into() }, vec![page.clone()])],
        );
        let o = run("How do we release?", &ask, &fetch).await.unwrap();
        assert_eq!((o.answer.as_str(), o.steps, o.finished), ("Tag from main.", 2, true));
        assert_eq!(o.sources, vec![page]);
        let second = prompts.lock().unwrap()[1].clone();
        assert!(second.contains("> search_pages for \"how do we release\"") && second.contains("[aaaaaaaa] (project page, kept up to date)"), "{second}");
        let shown = render(&o);
        assert!(shown.starts_with("Tag from main.\n\nSources:\n- [aaaaaaaa] (project page, kept up to date) Releases"), "{shown}");
    }

    #[tokio::test]
    async fn a_citation_of_something_never_retrieved_is_dropped() {
        let (ask, fetch, _) = scripted(vec!["{\"action\": \"answer\", \"text\": \"Probably Fridays.\", \"cites\": [\"deadbeef\", \"abc\"]}"], vec![]);
        let o = run("When do we ship?", &ask, &fetch).await.unwrap();
        assert!(o.sources.is_empty());
        assert!(render(&o).contains("(No memory was cited for this.)"));
    }

    #[tokio::test]
    async fn the_loop_is_bounded_and_the_last_call_must_answer() {
        let search = "{\"action\": \"recall\", \"query\": \"x\"}";
        let (ask, fetch, prompts) = scripted(vec![search, search, search, search, search, search, "{\"action\": \"answer\", \"text\": \"Partial.\", \"cites\": []}"], vec![]);
        let o = run("q", &ask, &fetch).await.unwrap();
        assert_eq!((o.answer.as_str(), o.steps), ("Partial.", MAX_STEPS + 1));
        assert!(!o.finished && render(&o).contains("partial"));
        assert!(prompts.lock().unwrap()[MAX_STEPS].contains("out of steps"));
        assert_eq!(prompts.lock().unwrap().len(), MAX_STEPS + 1, "never more than one extra call");
    }

    #[tokio::test]
    async fn an_expansion_uses_the_short_id_that_was_shown() {
        let obs = hit("cccccccc-2222", "belief drawn from several facts", "Ships on Fridays");
        let fact = hit("dddddddd-3333", "saved on purpose", "Deploys happen Friday afternoon");
        let (ask, fetch, _) = scripted(
            vec![
                "{\"action\": \"search_observations\", \"query\": \"ship\"}",
                "{\"action\": \"expand\", \"id\": \"cccccccc\"}",
                "{\"action\": \"expand\", \"id\": \"zzzzzzzz\"}",
                "{\"action\": \"answer\", \"text\": \"Fridays.\", \"cites\": [\"[dddddddd]\"]}",
            ],
            vec![
                (Step::Search { target: Target::Observations, query: "ship".into() }, vec![obs]),
                (Step::Expand { id: "cccccccc-2222".into() }, vec![fact.clone()]),
            ],
        );
        let o = run("When do we ship?", &ask, &fetch).await.unwrap();
        assert_eq!(o.sources, vec![fact]);
        assert_eq!(o.steps, 4);
    }

    #[tokio::test]
    async fn a_model_that_will_not_use_the_format_ends_the_loop_with_its_own_words() {
        let (ask, fetch, _) = scripted(vec!["I do not know."], vec![]);
        let o = run("q", &ask, &fetch).await.unwrap();
        assert_eq!((o.answer.as_str(), o.finished), ("I do not know.", false));
    }

    #[tokio::test]
    async fn a_failed_model_call_is_an_error_not_an_answer() {
        let (ask, fetch, _) = scripted(vec![], vec![]);
        assert!(run("q", &ask, &fetch).await.is_err());
    }
}

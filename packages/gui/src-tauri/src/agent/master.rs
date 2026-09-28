//! The master agent — one per project, briefed once at onboarding.
//!
//! ## What it is, concretely
//!
//! Not a separate process and not a second model. The master is the identity
//! the project's daily conversation runs under: a name, a role, and a charter,
//! all injected into the system prompt of that conversation on every turn (see
//! `directive::assemble_capability_blocks`, gated by `primary_agent`). Talking
//! to a project *is* talking to its master.
//!
//! Its continuity comes from three places, each already persistent: the
//! charter on disk, the daily handoff notes (`agent::days`), and the project's
//! own files. Nothing about it lives only in memory, so it survives restarts,
//! day rollovers and model switches unchanged.
//!
//! ## Why the charter is a file in the project
//!
//! `.viv/charter.md` and `.viv/agents/master.md` live in the project folder,
//! not in app data, because they are meant to be read and edited by the user
//! and — from Phase 3 on — by every agent in the project. The charter is the
//! first entry in what becomes the project ledger: the written-down intent
//! every agent defers to. The research this design rests on (Cognition's
//! "actions carry implicit decisions"; MAST's finding that most multi-agent
//! failures are organisational) is the reason intent is a document rather than
//! something each agent infers.
//!
//! ## Its team
//!
//! The master's specialists live beside it in `.viv/agents/` (see
//! `agent::team`). The role framing lists exactly the team that exists, active
//! and proposed separately: a master that believed it could delegate to agents
//! the user never approved would plan around them, and the plans would fail in
//! exactly the way MAST calls "disobeying the task specification".

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::agent::commands::AgentRegistry;

const VIV_DIR: &str = ".viv";

/// Cap on the charter as injected into the prompt. A charter is a page, not a
/// spec; anything longer is clipped, with a note, rather than silently eating
/// the context budget of every single turn.
const MAX_CHARTER_BYTES: usize = 12_000;

fn charter_path(root: &Path) -> PathBuf {
    root.join(VIV_DIR).join("charter.md")
}

fn master_path(root: &Path) -> PathBuf {
    root.join(VIV_DIR).join("agents").join("master.md")
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct MasterFrontmatter {
    name: Option<String>,
    created: Option<String>,
}

/// What `master.md`'s body says until the user writes their own. Recognised
/// so it is not injected into every turn as though it were an instruction.
const PLACEHOLDER_BODY: &str = "Additional standing instructions for this project's master agent go here.\n\
They are added to its role on every turn; the charter itself is in ../charter.md.\n";

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MasterInfo {
    pub name: String,
    pub charter: String,
    pub charter_path: String,
    pub created: Option<String>,
}

fn clip_to_boundary(s: &str, max: usize) -> (&str, bool) {
    if s.len() <= max {
        return (s, false);
    }
    let mut cut = max;
    while cut > 0 && !s.is_char_boundary(cut) {
        cut -= 1;
    }
    (&s[..cut], true)
}

/// `(identity, standing instructions, charter)` for the project at `root`.
async fn read_master(root: &Path) -> Option<(MasterFrontmatter, Option<String>, String)> {
    let charter = tokio::fs::read_to_string(charter_path(root)).await.ok()?;
    let (frontmatter, instructions) = match tokio::fs::read_to_string(master_path(root)).await {
        Ok(text) => {
            let (fm, body) = crate::coding_agent::tool_exec::parse_frontmatter::<MasterFrontmatter>(&text);
            // Compared with line endings normalised: an editor on Windows will
            // happily resave the untouched placeholder as CRLF.
            let body = body.replace("\r\n", "\n");
            let is_placeholder = body.trim() == PLACEHOLDER_BODY.trim();
            (fm, (!is_placeholder && !body.trim().is_empty()).then(|| body.trim().to_string()))
        }
        // A charter without an agent file still defines the project — the
        // user may have written one by hand. Fall back to a default identity
        // rather than ignoring the charter.
        Err(_) => (MasterFrontmatter::default(), None),
    };
    Some((frontmatter, instructions, charter))
}

/// The system-prompt block for a project's master, or `None` when the
/// project has no charter.
pub async fn prompt_block(root: &str) -> Option<String> {
    let root = Path::new(root);
    let (fm, instructions, charter) = read_master(root).await?;
    let name = fm.name.filter(|n| !n.trim().is_empty()).unwrap_or_else(|| "the master agent".to_string());
    let (charter_text, clipped) = clip_to_boundary(charter.trim(), MAX_CHARTER_BYTES);
    let team = crate::agent::team::prompt_section(root);

    let mut block = format!(
        "<project_master>\n\
You are {name}, this project's master agent. The user briefed you once, at onboarding, and \
you carry the project from there: you plan it, build it, and keep it true to the charter below.\n\
\n\
How you work:\n\
- The charter is the source of truth for what this project is. When the user changes \
direction, update `.viv/charter.md` in the same turn, so the change outlives this \
conversation — tomorrow's conversation starts from the charter and a short handoff, not \
today's transcript.\n\
- Write down decisions that shape the rest of the project (stack, structure, conventions, \
data shapes) in the charter's Decisions section, with the reason. An unwritten decision gets \
silently contradicted later.\n\
- Before calling something done, run it and show the evidence: the test output, the build \
result, the command you ran. Say plainly when something is unverified.\n\
- If a request conflicts with the charter, say so and ask which should win rather than \
quietly picking one.\n\
\n\
{team}\
\n\
--- charter (.viv/charter.md) ---\n{charter_text}\n"
    );
    if clipped {
        block.push_str("\n[charter clipped here — read .viv/charter.md for the rest]\n");
    }
    if let Some(instructions) = instructions {
        block.push_str(&format!("\n--- standing instructions (.viv/agents/master.md) ---\n{instructions}\n"));
    }
    block.push_str("</project_master>");
    Some(block)
}

const CHARTER_SYSTEM_PROMPT: &str = "\
You turn the answers from a project onboarding interview into a project charter: the \
founding document an AI engineering agent will work from for the life of the project. The \
answers are structured — project type, users, features, technology choices, quality bar, \
constraints — plus free-text notes and answers to follow-up questions. Every fact the person \
gave must appear in the charter; nothing may be dropped because it seemed minor.

Where the person explicitly chose a technology, record it under Decisions as \
\"<choice> — chosen at onboarding\". Where they left a choice to the agent, say so under \
Technical direction rather than picking one yourself.

Write Markdown with exactly these sections, in this order:

# <Project name>
## Purpose — what this is and the problem it solves, in two or three sentences.
## Users — who it is for.
## Scope — the features and capabilities in scope, as a bullet list.
## Out of scope — what it deliberately will not do. Infer sensible exclusions if none were given.
## Technical direction — stack, platforms, architecture preferences. Only what the person said \
or clearly implied; mark anything you inferred with \"(assumed)\".
## Constraints — deadlines, budgets, performance, compatibility, licensing.
## Definition of done — concrete, checkable criteria for a first usable version.
## Decisions — technology choices the person made explicitly, one bullet each; \
\"_None recorded yet._\" if they made none.
## Open questions — what the agent must ask before or during building.

Rules: never invent facts the person did not give — put unknowns under Open questions instead. \
Be concrete and brief; the whole charter should fit on one page. Output only the Markdown.";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CharterDraft {
    pub charter: String,
    /// False when the model could not be reached and this is the template
    /// filled from the raw description.
    pub drafted_by_model: bool,
    /// Why the model draft failed, when it did — shown to the user so a broken
    /// provider isn't mistaken for a bad description.
    pub error: Option<String>,
}

fn template_charter(name: &str, description: &str) -> String {
    format!(
        "# {name}\n\n\
## Purpose\n{description}\n\n\
## Users\n_Who is this for?_\n\n\
## Scope\n- \n\n\
## Out of scope\n- \n\n\
## Technical direction\n_Stack, platforms, architecture preferences._\n\n\
## Constraints\n_Deadlines, performance, compatibility._\n\n\
## Definition of done\n- \n\n\
## Decisions\n_None recorded yet._\n\n\
## Open questions\n- \n",
        description = description.trim()
    )
}

/// Draft a charter from the user's description, using their selected model.
///
/// Never fails outright: if the model is unreachable the draft falls back to
/// a template holding the description verbatim, and the error comes back
/// alongside it. Onboarding must not dead-end on a provider outage — the
/// charter is editable either way.
#[tauri::command]
pub async fn cmd_draft_charter(
    app: tauri::AppHandle,
    name: String,
    description: String,
    registry: State<'_, AgentRegistry>,
) -> Result<CharterDraft, String> {
    let name = name.trim().to_string();
    let description = description.trim().to_string();
    if description.is_empty() {
        return Err("Describe the project first.".to_string());
    }
    let model = registry.resolve(None).await.agent_state.model().await;
    let prompt = format!("Project name: {name}\n\nDescription:\n{description}");
    let call = crate::ai::completion::complete_text(&app, &model, CHARTER_SYSTEM_PROMPT, &prompt);

    let failed = |error: String| CharterDraft {
        charter: template_charter(&name, &description),
        drafted_by_model: false,
        error: Some(error),
    };
    match tokio::time::timeout(std::time::Duration::from_secs(90), call).await {
        Ok(Ok(text)) => {
            let text = strip_code_fence(text.trim());
            if text.is_empty() {
                Ok(failed("The model returned an empty draft.".to_string()))
            } else {
                Ok(CharterDraft { charter: text.to_string(), drafted_by_model: true, error: None })
            }
        }
        Ok(Err(e)) => Ok(failed(e)),
        Err(_) => Ok(failed("The model took longer than 90 seconds.".to_string())),
    }
}

/// Models often wrap Markdown output in a ```markdown fence despite being told
/// not to; the fence would otherwise end up inside the charter file.
fn strip_code_fence(s: &str) -> &str {
    let Some(rest) = s.strip_prefix("```") else { return s };
    let body = rest.split_once('\n').map(|(_, b)| b).unwrap_or("");
    body.strip_suffix("```").map(str::trim_end).unwrap_or(body).trim()
}

/// Write the charter and the master's identity into the project folder.
///
/// Refuses to replace an existing charter: re-running onboarding by accident
/// must not wipe a document the master and the user have been editing for
/// weeks. Editing an existing charter is done by editing the file.
#[tauri::command]
pub async fn cmd_create_master(
    root: String,
    name: String,
    charter: String,
    // Standing instructions from onboarding's "how should it work" step —
    // autonomy, check-ins, git habits. Written as the body of `master.md`, so
    // they reach the master every turn and stay editable by hand.
    instructions: Option<String>,
) -> Result<(), String> {
    let root_path = Path::new(&root);
    if !root_path.is_dir() {
        return Err(format!("Project folder does not exist: {root}"));
    }
    let charter = charter.trim();
    if charter.is_empty() {
        return Err("The charter is empty.".to_string());
    }
    let charter_file = charter_path(root_path);
    if charter_file.exists() {
        return Err("This project already has a charter at .viv/charter.md — edit that file instead.".to_string());
    }
    let name = name.trim();
    let name = if name.is_empty() { "Master" } else { name };

    let agent_file = master_path(root_path);
    std::fs::create_dir_all(agent_file.parent().expect("agents dir has a parent"))
        .map_err(|e| format!("Failed to create .viv folder: {e}"))?;

    // The YAML name is quoted: a name like "Ops: Lead" would otherwise parse
    // as a nested mapping and silently lose the agent's identity.
    let body = instructions
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .map(|s| format!("{s}\n"))
        .unwrap_or_else(|| PLACEHOLDER_BODY.to_string());
    let agent = format!(
        "---\nname: {}\ncreated: {}\n---\n{body}",
        serde_json::to_string(name).unwrap_or_else(|_| "\"Master\"".to_string()),
        crate::agent::days::today(),
    );
    std::fs::write(&agent_file, agent).map_err(|e| format!("Failed to write master agent: {e}"))?;
    std::fs::write(&charter_file, format!("{charter}\n")).map_err(|e| format!("Failed to write charter: {e}"))?;
    Ok(())
}

/// The project's master, if it has one.
#[tauri::command]
pub async fn cmd_get_master(root: String) -> Result<Option<MasterInfo>, String> {
    let root_path = Path::new(&root);
    let Some((fm, _, charter)) = read_master(root_path).await else { return Ok(None) };
    Ok(Some(MasterInfo {
        name: fm.name.filter(|n| !n.trim().is_empty()).unwrap_or_else(|| "Master".to_string()),
        charter,
        charter_path: charter_path(root_path).to_string_lossy().to_string(),
        created: fm.created,
    }))
}

const QUESTIONS_SYSTEM_PROMPT: &str = "\
You are interviewing someone about a software project they want an AI engineering agent to \
build. You have their answers so far. Ask the follow-up questions whose answers would most \
change how the project gets built — the gaps and ambiguities a senior engineer would want \
settled before writing any code. Never ask about anything they already answered.

Good questions are specific to this project: data it must store and where it comes from, \
integrations with outside services, edge cases in the core features, who can see or change \
what, what happens offline or on failure, migration from something that exists today.

Ask between 3 and 6 questions. For each, offer 2 to 5 short suggested answers the person can \
pick from, written as they would say them. Set \"multi\" to true when several answers can \
apply at once.

Respond with ONLY a JSON array, no prose and no code fence:
[{\"question\": \"...\", \"options\": [\"...\", \"...\"], \"multi\": false}]";

#[derive(Debug, Serialize, Deserialize)]
pub struct FollowUpQuestion {
    pub question: String,
    #[serde(default)]
    pub options: Vec<String>,
    #[serde(default)]
    pub multi: bool,
}

/// Pull the JSON array out of a model reply that may have wrapped it in a
/// fence or a sentence despite being told not to.
fn extract_json_array(reply: &str) -> Option<&str> {
    let start = reply.find('[')?;
    let end = reply.rfind(']')?;
    (end > start).then(|| &reply[start..=end])
}

/// Follow-up questions tailored to what the user has said so far.
///
/// This is what turns onboarding from a form into an interview: a fixed form
/// can only ask what every project has in common, and the questions that
/// decide how a project is actually built are specific to it.
#[tauri::command]
pub async fn cmd_onboarding_questions(
    app: tauri::AppHandle,
    brief: String,
    registry: State<'_, AgentRegistry>,
) -> Result<Vec<FollowUpQuestion>, String> {
    let model = registry.resolve(None).await.agent_state.model().await;
    let call = crate::ai::completion::complete_text(&app, &model, QUESTIONS_SYSTEM_PROMPT, &brief);
    let reply = match tokio::time::timeout(std::time::Duration::from_secs(60), call).await {
        Ok(Ok(text)) => text,
        Ok(Err(e)) => return Err(e),
        Err(_) => return Err("The model took longer than 60 seconds.".to_string()),
    };
    let json = extract_json_array(&reply).ok_or("The model's reply had no questions in it.")?;
    let questions: Vec<FollowUpQuestion> =
        serde_json::from_str(json).map_err(|e| format!("The model's questions weren't readable: {e}"))?;
    Ok(questions
        .into_iter()
        .filter(|q| !q.question.trim().is_empty())
        .take(8)
        .map(|mut q| {
            q.options.retain(|o| !o.trim().is_empty());
            q.options.truncate(6);
            q
        })
        .collect())
}

/// Open one of a project's agent files (`.viv/**/*.md`) in the user's own
/// Markdown editor.
///
/// Deliberately narrow. "Open this path with its default app" is how you run
/// an executable on every desktop OS — `explorer foo.exe` launches it — so a
/// general version would let anything that can call a command start any
/// program on disk. The charter and agent files only ever need `.md` inside a
/// `.viv` folder, so that is all this accepts.
#[tauri::command]
pub async fn cmd_open_agent_file(path: String) -> Result<(), String> {
    let p = Path::new(&path);
    let is_md = p.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("md"));
    let in_viv = p.components().any(|c| c.as_os_str() == VIV_DIR);
    if !is_md || !in_viv || !p.is_file() {
        return Err(format!("Only Markdown files inside a project's .viv folder can be opened this way: {path}"));
    }

    #[cfg(windows)]
    let result = std::process::Command::new("explorer").arg(p).spawn();
    #[cfg(target_os = "macos")]
    let result = std::process::Command::new("open").arg(p).spawn();
    #[cfg(all(not(windows), not(target_os = "macos")))]
    let result = std::process::Command::new("xdg-open").arg(p).spawn();

    result.map(|_| ()).map_err(|e| format!("Couldn't open {path}: {e}"))
}

/// The first message of a newly onboarded project, sent on the user's behalf
/// when they choose "Create and start".
pub const KICKOFF_PROMPT: &str = "\
You've just been briefed with the charter for this project. Start it:

1. Read the charter and restate, in a few lines, what you understand we're building.
2. Ask any open question you genuinely can't proceed without — only those. Assume sensibly \
for the rest and say what you assumed.
3. Propose a short build plan: the project structure and the first milestones.
4. If the folder is empty, scaffold the project to match, and show that it builds or runs.
5. Propose the specialists this project needs with `hire_agent` — usually two to four, each with \
a clear area and the paths it owns. The user approves each one before it can take work.

Record the decisions you make in the charter as you go.";

#[tauri::command]
pub fn cmd_master_kickoff_prompt() -> &'static str {
    KICKOFF_PROMPT
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_a_markdown_fence() {
        assert_eq!(strip_code_fence("```markdown\n# A\ntext\n```"), "# A\ntext");
        assert_eq!(strip_code_fence("```\n# A\n```"), "# A");
        assert_eq!(strip_code_fence("# A\nno fence"), "# A\nno fence");
    }

    #[test]
    fn clipping_respects_char_boundaries() {
        let s = "héllo wörld";
        let (clipped, was) = clip_to_boundary(s, 2);
        assert!(was);
        assert!(s.starts_with(clipped));
        assert_eq!(clip_to_boundary("short", 100), ("short", false));
    }

    #[tokio::test]
    async fn create_then_read_then_refuse_to_overwrite() {
        let dir = std::env::temp_dir().join(format!("viv-master-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let root = dir.to_string_lossy().to_string();

        // A name with a colon — would break unquoted YAML.
        cmd_create_master(root.clone(), "Ops: Lead".into(), "# Demo\n## Purpose\nA test.".into(), None)
            .await
            .expect("create");
        let info = cmd_get_master(root.clone()).await.unwrap().expect("has master");
        assert_eq!(info.name, "Ops: Lead");
        assert!(info.charter.contains("A test."));

        let block = prompt_block(&root).await.expect("block");
        assert!(block.starts_with("<project_master>"));
        assert!(block.contains("You are Ops: Lead"));
        assert!(block.contains("A test."));
        assert!(block.ends_with("</project_master>"));
        // The untouched placeholder is not an instruction.
        assert!(!block.contains("standing instructions"));

        // Instructions the user writes are.
        let agent_file = master_path(Path::new(&root));
        let edited = std::fs::read_to_string(&agent_file).unwrap().replace(PLACEHOLDER_BODY, "Always use pnpm.\n");
        std::fs::write(&agent_file, edited).unwrap();
        let block = prompt_block(&root).await.expect("block");
        assert!(block.contains("Always use pnpm."));

        // Onboarding twice must not wipe an edited charter.
        let second = cmd_create_master(root.clone(), "X".into(), "# Other".into(), None).await;
        assert!(second.is_err());
        assert!(cmd_get_master(root.clone()).await.unwrap().unwrap().charter.contains("A test."));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn follow_up_questions_survive_a_chatty_model() {
        // Told to reply with only JSON; models add a sentence or a fence anyway.
        let reply = "Here are my questions:\n```json\n[{\"question\": \"Who logs in?\", \"options\": [\"Only me\", \"Anyone\"], \"multi\": false}]\n```";
        let json = extract_json_array(reply).expect("array found");
        let qs: Vec<FollowUpQuestion> = serde_json::from_str(json).unwrap();
        assert_eq!(qs[0].question, "Who logs in?");
        assert_eq!(qs[0].options, vec!["Only me", "Anyone"]);
        assert!(extract_json_array("no questions here").is_none());
    }

    #[tokio::test]
    async fn onboarding_instructions_replace_the_placeholder() {
        let dir = std::env::temp_dir().join(format!("viv-master-instr-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let root = dir.to_string_lossy().to_string();
        cmd_create_master(root.clone(), "Lead".into(), "# P".into(), Some("Ask before adding a dependency.".into()))
            .await
            .unwrap();
        let block = prompt_block(&root).await.unwrap();
        assert!(block.contains("standing instructions"));
        assert!(block.contains("Ask before adding a dependency."));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn no_charter_means_no_block() {
        let dir = std::env::temp_dir().join(format!("viv-master-none-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert!(prompt_block(&dir.to_string_lossy()).await.is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}

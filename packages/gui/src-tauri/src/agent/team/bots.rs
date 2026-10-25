//! Bots: persistent teammates, the way Grok Bots and OpenAI Dots work, on
//! the user's own machine.
//!
//! A bot is one of the project's agents (`.viv/agents/<id>.md`: name, job,
//! standing rules, model, tools) given a life of its own:
//!
//! - **Its own conversation and memory.** Every turn continues the bot's own
//!   thread (`.viv/bots/<id>/thread.json`), and it keeps durable notes in
//!   `.viv/bots/<id>/memory.md`, which the user can read and edit. Memory is
//!   a starting point, not the truth: bots are told to re-check current data
//!   before acting on what they remember.
//! - **A shared room.** The team chat (`.viv/bots/room.jsonl`) is one visible
//!   conversation between the user and every bot. A message goes to the bots
//!   it `@mentions`, to all of them with `@everyone`, and otherwise to
//!   whoever a quick routing call judges should answer. Bots hand work to
//!   each other by mentioning one another in the room.
//! - **Direct messages.** Each bot can also be talked to alone.
//! - **Routines.** Scheduled prompts per bot (cron), each keeping its 20
//!   most recent runs, that work while the user is elsewhere.
//! - **Always-on check-ins.** Optionally a bot looks in on the project every
//!   few hours in read-only "proactive" mode and posts only when it found
//!   something worth the user's attention.
//! - **Rules.** Per kind of action (changing files, running commands, using
//!   the computer, the web, acting outside the machine): act without
//!   asking, act if the user asked for it, ask first, or hand it to the
//!   user. Proactive check-ins are read-only whatever the rules say.
//! - **Needs you.** A bot that needs a decision says so in the room, and the
//!   user gets a notification.
//!
//! Unlike Dots and Grok Bots, which run on a cloud computer each, these run
//! here, with the app's tools, while the app is running (it keeps running in
//! the tray when its window is closed).

use crate::agent::runtime::commands::ImageInput;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use tauri::{Emitter, Manager};

use crate::agent::runtime::events::AgentEvent;
use crate::agent::team::TeamAgent;
use crate::agent::runtime::types::{AgentContext, AgentMessage, MessageContent, PersonaMode, QueueMode};

// ── Rules ────────────────────────────────────────────────────────────────

/// What a bot may do on its own, per kind of action (OpenAI's Custom Rules).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    /// Take action without asking.
    Auto,
    /// Take action if the request asked for it; otherwise ask.
    IfRequested,
    /// Ask before taking action.
    Ask,
    /// Don't do it: hand it to the user.
    HandOff,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Category {
    Files,
    Commands,
    Computer,
    Web,
    Outside,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BotRules {
    pub files: Level,
    pub commands: Level,
    pub computer: Level,
    pub web: Level,
    pub outside: Level,
}

impl Default for BotRules {
    fn default() -> Self {
        Self {
            files: Level::IfRequested,
            commands: Level::Ask,
            computer: Level::Ask,
            web: Level::Auto,
            outside: Level::Ask,
        }
    }
}

impl BotRules {
    fn level(&self, c: Category) -> Level {
        match c {
            Category::Files => self.files,
            Category::Commands => self.commands,
            Category::Computer => self.computer,
            Category::Web => self.web,
            Category::Outside => self.outside,
        }
    }
}

/// Which kind of action a tool is. `None`: reading, planning and talking,
/// which never need a rule.
pub fn category(tool: &str) -> Option<Category> {
    Some(match tool {
        "write" | "edit" | "apply_patch" | "notebook_edit" | "edit_document" | "create_document" => Category::Files,
        "bash" | "powershell" | "terminal" | "run_script"
        | "sandbox_execute" | "math" | "create_tool" | "delete_tool" => Category::Commands,
        "dip" | "computer" | "user_browser_driver" => Category::Computer,
        "vault" | "nebius_job" => Category::Outside,
        "web_search" | "web_fetch" | "web_preview" | "generate_image" => Category::Web,
        "github_act" | "github_api" | "connect_mcp_server" | "create_automation" | "email" | "phone" => Category::Outside,
        t if t.starts_with("browser_") => Category::Web,
        t if t.starts_with("mcp__") => Category::Outside,
        _ if crate::coding_agent::custom_tools::custom_tool_names().contains(tool) => Category::Commands,
        _ => return None,
    })
}

/// Words that show a request asked for a kind of action, for "if requested".
fn asked_for(c: Category, request: &str) -> bool {
    let r = request.to_lowercase();
    let words: &[&str] = match c {
        Category::Files => &["write", "edit", "change", "fix", "create", "add", "update", "save", "rename", "refactor", "implement", "file"],
        Category::Commands => &["run", "install", "build", "test", "execute", "command", "script", "compile", "deploy"],
        Category::Computer => &["click", "type", "open", "computer", "app", "screen", "window", "desktop"],
        Category::Web => &["search", "look up", "browse", "web", "fetch", "site", "online", "research"],
        Category::Outside => &["post", "send", "publish", "merge", "issue", "pull request", "pr ", "release", "email", "message"],
    };
    words.iter().any(|w| r.contains(w))
}

/// Why a bot is running: the user asked, a routine fired, or a check-in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    User,
    Routine,
    Proactive,
    /// Email or a text that arrived at the bot's own address
    /// (`agent::team::contact`). From outside, so never "what the user asked".
    Inbound,
}

/// Set on a bot's run so the tool gate applies its rules.
#[derive(Debug, Clone)]
pub struct BotPolicy {
    pub bot_name: String,
    pub rules: BotRules,
    pub origin: Origin,
    /// What started this run, for "if requested".
    pub request: String,
    /// Where the bot lives and which it is, for its own email and phone.
    pub root: String,
    pub bot_id: String,
}

pub enum Decision {
    Run,
    Ask(String),
    Refuse(String),
}

pub fn decide(policy: &BotPolicy, tool: &str) -> Decision {
    let Some(c) = category(tool) else { return Decision::Run };
    // A check-in nobody asked for may look, never touch (OpenAI's proactive
    // research rule). Reading the web is looking.
    if policy.origin == Origin::Proactive && c != Category::Web {
        return Decision::Refuse(format!(
            "'{tool}' isn't available during a check-in: check-ins are read-only. If it's worth doing, say so in your post and the user can ask you."
        ));
    }
    match policy.rules.level(c) {
        Level::Auto => Decision::Run,
        // A message from outside asking for something is not the user
        // asking for it.
        Level::IfRequested if policy.origin != Origin::Inbound && asked_for(c, &policy.request) => Decision::Run,
        Level::IfRequested | Level::Ask => Decision::Ask(format!("{}'s rules say to ask before this", policy.bot_name)),
        Level::HandOff => Decision::Refuse(format!(
            "Your rules say to hand '{tool}' to the user rather than do it. Start your reply with \"NEEDS YOU:\" and say exactly what you need them to do."
        )),
    }
}

/// `decide`, knowing the call's arguments: reading the bot's own inbox or
/// texts is looking, not acting, whatever the tool's category.
pub fn decide_call(policy: &BotPolicy, tool: &str, args: &serde_json::Value) -> Decision {
    if crate::agent::team::contact::reads_only(tool, args) {
        return Decision::Run;
    }
    decide(policy, tool)
}

// ── General's agents ─────────────────────────────────────────────────────

/// Where the General tab's agents live: a folder of their own in Vivianne's
/// home, used as their "project" by everything here (agents, threads,
/// memory, routines, rules, contact details).
pub fn general_root() -> Option<String> {
    let dir = crate::agent::system::paths::viv_home().ok()?.join("agents");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir.display().to_string())
}

/// Every folder whose agents run on their own: General's, and team projects.
pub fn bot_roots() -> Vec<String> {
    let mut roots: Vec<String> = general_root().into_iter().collect();
    for project in crate::agent::sessions::projects::list_all() {
        let Some(root) = project.path.clone().filter(|p| !p.trim().is_empty()) else { continue };
        if crate::agent::sessions::projects::mode_for_workspace(&root).as_deref() == Some("team") && !roots.contains(&root) {
            roots.push(root);
        }
    }
    roots
}

pub fn active_bot_ids(root: &str) -> Vec<String> {
    active_bots(Path::new(root)).into_iter().map(|a| a.id).collect()
}

/// Hand a bot an email or text that arrived for it: shown in its DM, and
/// answered there under its rules.
pub fn deliver_inbound(app: &tauri::AppHandle, root: &str, id: &str, text: String) {
    post(app, root, id, "inbox", &text, "note", Vec::new());
    dispatch(app, root, id, Origin::Inbound, text, Target::Dm);
}

#[tauri::command]
pub async fn cmd_general_agents_root() -> Result<String, String> {
    general_root().ok_or_else(|| "Couldn't create the agents folder.".to_string())
}

// ── Storage ──────────────────────────────────────────────────────────────

fn bots_dir(root: &Path) -> PathBuf {
    root.join(".viv").join("bots")
}

fn bot_dir(root: &Path, id: &str) -> PathBuf {
    bots_dir(root).join(id)
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

fn read_json<T: serde::de::DeserializeOwned + Default>(path: &Path) -> T {
    std::fs::read_to_string(path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

use crate::agent::system::paths::write_json_atomic as write_json;

/// Serialises appends to room and DM logs.
static LOG_LOCK: Mutex<()> = Mutex::new(());

fn append_line<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    use std::io::Write;
    let _g = LOG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(path).map_err(|e| e.to_string())?;
    writeln!(f, "{}", serde_json::to_string(value).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}

fn read_lines<T: serde::de::DeserializeOwned>(path: &Path, last: usize) -> Vec<T> {
    let Ok(text) = std::fs::read_to_string(path) else { return Vec::new() };
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    let skip = lines.len().saturating_sub(last);
    lines[skip..].iter().filter_map(|l| serde_json::from_str(l).ok()).collect()
}

/// Per-bot settings beside its definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BotSettings {
    #[serde(default)]
    pub rules: BotRules,
    #[serde(default)]
    pub paused: bool,
    /// Look in on the project every `checkin_hours` (read-only).
    #[serde(default)]
    pub always_on: bool,
    #[serde(default = "default_checkin_hours")]
    pub checkin_hours: u32,
    #[serde(default)]
    pub last_checkin: Option<u64>,
}

fn default_checkin_hours() -> u32 {
    4
}

impl Default for BotSettings {
    fn default() -> Self {
        Self { rules: BotRules::default(), paused: false, always_on: false, checkin_hours: 4, last_checkin: None }
    }
}

fn load_settings(root: &Path, id: &str) -> BotSettings {
    read_json(&bot_dir(root, id).join("settings.json"))
}

fn save_settings(root: &Path, id: &str, s: &BotSettings) -> Result<(), String> {
    write_json(&bot_dir(root, id).join("settings.json"), s)
}

pub const MAX_ROUTINES: usize = 50;
const RUNS_KEPT: usize = 20;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoutineRun {
    pub at: u64,
    pub ok: bool,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Routine {
    #[serde(default)]
    pub id: String,
    pub title: String,
    pub prompt: String,
    /// Cron, 5 fields (`0 9 * * 1-5` = 9am on weekdays).
    pub cron: String,
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default)]
    pub next_run: Option<u64>,
    #[serde(default)]
    pub runs: Vec<RoutineRun>,
}

fn yes() -> bool {
    true
}

fn load_routines(root: &Path, id: &str) -> Vec<Routine> {
    read_json(&bot_dir(root, id).join("routines.json"))
}

fn save_routines(root: &Path, id: &str, r: &[Routine]) -> Result<(), String> {
    write_json(&bot_dir(root, id).join("routines.json"), &r.to_vec())
}

fn next_run(cron: &str) -> Option<u64> {
    crate::automation::next_run_after(cron, chrono::Utc::now())
}

/// One message in the room or a DM.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessage {
    pub id: String,
    /// `user`, a bot id, or `system`.
    pub from: String,
    pub text: String,
    pub ts: u64,
    /// `message` | `needs_you` | `routine` | `checkin` | `note`
    #[serde(default = "kind_message")]
    pub kind: String,
    #[serde(default)]
    pub mentions: Vec<String>,
    /// What the agent did to get to this reply, step by step (tool calls),
    /// for the "N steps" record under it in chat.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub steps: Vec<BotStep>,
    /// What the user attached to this message, by name (the content went to
    /// the agent; the chat shows chips).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<AttachmentRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentRef {
    pub name: String,
    /// `image` | `file`
    pub kind: String,
}

/// A text file attached to a DM (read on the user's side, like the main chat).
#[derive(Debug, Clone, Deserialize)]
pub struct FileInput {
    pub name: String,
    pub content: String,
    #[serde(default)]
    pub truncated: bool,
}

/// One thing an agent did during a turn: a tool call, in a few words.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BotStep {
    pub id: String,
    pub tool: String,
    /// What it was, for a person: "Read src/main.rs", "Searched the web for …".
    pub label: String,
    /// `running` | `done` | `error`
    pub state: String,
    /// The start of the result (or the error), when there is one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// Steps kept with a reply, at most: a long run keeps its last ones.
const MAX_STEPS: usize = 60;

/// "Read src/main.rs" from a tool call: its name in words and the argument
/// that says what it was about.
fn step_label(tool: &str, args: &serde_json::Value) -> String {
    if tool == "dip" || tool == "computer" {
        return crate::activity::describe(args);
    }
    let arg = ["path", "file_path", "query", "url", "command", "pattern", "title", "name", "prompt", "description", "to", "text"]
        .iter()
        .find_map(|k| args.get(*k).and_then(|v| v.as_str()))
        .map(|v| v.lines().next().unwrap_or("").trim().to_string())
        .filter(|v| !v.is_empty());
    let verb = match tool {
        "read" => "Read",
        "write" => "Wrote",
        "edit" | "multi_edit" => "Edited",
        "ls" => "Listed",
        "glob" => "Found files",
        "grep" => "Searched files for",
        "bash" | "powershell" | "sandbox_execute" => "Ran",
        "web_search" => "Searched the web for",
        "web_fetch" | "web_preview" => "Opened",
        "create_document" => "Made a document",
        "edit_document" => "Edited a document",
        "generate_image" => "Made an image",
        "task" => "Asked a subagent to",
        "email" => "Email",
        "phone" => "Phone",
        "recall" => "Recalled",
        "remember" => "Remembered",
        other => return match arg {
            Some(a) => format!("{} {}", other.replace('_', " "), clip(&a, 80)),
            None => other.replace('_', " "),
        },
    };
    match arg {
        Some(a) => format!("{verb} {}", clip(&a, 80)),
        None => verb.to_string(),
    }
}

fn clip(s: &str, n: usize) -> String {
    let mut out: String = s.chars().take(n).collect();
    if s.chars().count() > n {
        out.push('…');
    }
    out
}

fn result_text(r: &crate::agent::runtime::types::AgentToolResult) -> String {
    r.content
        .iter()
        .filter_map(|c| match c {
            crate::agent::runtime::types::MessageContent::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn emit_steps(app: &tauri::AppHandle, root: &str, id: &str) {
    let key = (root.to_string(), id.to_string());
    let (steps, channel) = {
        let l = live();
        (l.steps.get(&key).cloned().unwrap_or_default(), l.channel.get(&key).cloned().unwrap_or_else(|| id.to_string()))
    };
    let _ = app.emit("bots:steps", serde_json::json!({ "root": root, "botId": id, "channel": channel, "steps": steps }));
}

fn kind_message() -> String {
    "message".into()
}

fn room_path(root: &Path) -> PathBuf {
    bots_dir(root).join("room.jsonl")
}

fn dm_path(root: &Path, id: &str) -> PathBuf {
    bot_dir(root, id).join("dm.jsonl")
}

fn memory_path(root: &Path, id: &str) -> PathBuf {
    bot_dir(root, id).join("memory.md")
}

fn thread_path(root: &Path, id: &str) -> PathBuf {
    bot_dir(root, id).join("thread.json")
}

// ── Live state ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BotStatus {
    pub root: String,
    pub bot_id: String,
    /// `idle` | `working` | `paused`
    pub state: String,
    /// What it's doing right now, in a few words.
    pub activity: Option<String>,
}

#[derive(Default)]
struct Live {
    /// (root, bot) pairs with a run in flight, and the run's abort flag.
    running: HashMap<(String, String), Arc<AtomicBool>>,
    activity: HashMap<(String, String), String>,
    /// The steps of the run in flight, shown live in its conversation.
    steps: HashMap<(String, String), Vec<BotStep>>,
    /// Where the run in flight will reply: `room` or the bot's own DM.
    channel: HashMap<(String, String), String>,
    /// Requests that arrived while the bot was busy, run after it finishes.
    queued: HashMap<(String, String), Vec<(Origin, String, Target, Vec<ImageInput>)>>,
}

static LIVE: std::sync::LazyLock<Mutex<Live>> = std::sync::LazyLock::new(|| Mutex::new(Live::default()));

fn live() -> std::sync::MutexGuard<'static, Live> {
    LIVE.lock().unwrap_or_else(|e| e.into_inner())
}

fn status_of(root: &str, id: &str) -> BotStatus {
    let key = (root.to_string(), id.to_string());
    let l = live();
    let paused = load_settings(Path::new(root), id).paused;
    BotStatus {
        root: root.into(),
        bot_id: id.into(),
        state: if l.running.contains_key(&key) { "working" } else if paused { "paused" } else { "idle" }.into(),
        activity: l.activity.get(&key).cloned(),
    }
}

fn emit_status(app: &tauri::AppHandle, root: &str, id: &str) {
    let _ = app.emit("bots:status", status_of(root, id));
}

fn emit_message(app: &tauri::AppHandle, root: &str, channel: &str, msg: &ChatMessage) {
    let _ = app.emit("bots:message", serde_json::json!({ "root": root, "channel": channel, "message": msg }));
}

// ── Running a bot ────────────────────────────────────────────────────────

/// Where a run's reply goes: the room, or the bot's DM.
#[derive(Debug, Clone, PartialEq)]
enum Target {
    Room { hops: u32 },
    Dm,
}

const MAX_HOPS: u32 = 4;
const THREAD_KEPT: usize = 80;
/// Past this many messages the thread is cut back to `THREAD_KEPT`.
const THREAD_MAX: usize = 160;
const ROOM_IN_PROMPT: usize = 30;

fn active_bots(root: &Path) -> Vec<TeamAgent> {
    crate::agent::team::list(root).into_iter().filter(|a| a.status == crate::agent::team::STATUS_ACTIVE).collect()
}

fn find_bot(root: &Path, id: &str) -> Option<TeamAgent> {
    active_bots(root).into_iter().find(|a| a.id == id)
}

fn identity(root: &Path, bot: &TeamAgent, settings: &BotSettings, team: &[TeamAgent]) -> String {
    let mut p = format!(
        "You are {name}, a persistent teammate on this project (@{id}). You are not a one-off assistant: you have your own \
         ongoing conversation, your own memory, and a job you keep doing over time.\n\nYour job: {role}\n",
        name = bot.name,
        id = bot.id,
        role = if bot.role.trim().is_empty() { "help the user with this project" } else { bot.role.trim() }
    );
    if !bot.instructions.trim().is_empty() {
        p.push_str(&format!("\n## Your standing rules\n\n{}\n", bot.instructions.trim()));
    }
    let others: Vec<String> = team.iter().filter(|a| a.id != bot.id).map(|a| format!("- @{} ({}): {}", a.id, a.name, a.role)).collect();
    if !others.is_empty() {
        p.push_str(&format!("\n## Your teammates\n\n{}\n", others.join("\n")));
    }
    p.push_str(
        "\n## How the team works\n\n\
         The team talks in one shared room with the user. Your reply is your post: write it to the room, briefly, \
         like a colleague. To hand work to a teammate, mention them (@id) with exactly what you need; only mention \
         someone when you are handing them something. One owner per piece of work: if a teammate owns it, hand it over \
         instead of doing it yourself.\n\
         When you need a decision, approval or something only the user can do, start your reply with \"NEEDS YOU:\" \
         and say exactly what you need.\n\
         Telling you to stop ends your work; it does not undo what you already did, so say plainly what you changed.\n\n\
         ## How you answer\n\n\
         Chat and quick answers are plain, short text. When a result has shape (the weather, a comparison, a trip or a \
         plan, search results, a status or a routine's results, options to pick from, a form) answer with one plain \
         sentence and then an OpenUI card: openui-lang in a ```openui-lang fence, which the chat draws as a live, \
         interactive card. This replaces the ```ui summary cards described elsewhere: in this conversation use OpenUI. \
         Read the `openui` guide (guide tool) before your first card, for the language and its components. Every number, \
         name, price and date in a card comes from what you looked up in this conversation: never invent data to fill a \
         layout. Buttons with @ToAssistant send their message back to you here.\n\n\
         ## Delivering work\n\n\
         When the user asks for a thing (a PDF, a document, a sheet, an image), make it with your tools, read it back to \
         check it has what it should, and only then say it's done, naming the file: the chat shows it. Don't hand the \
         user a plan for work you can simply do; do the work.\n",
    );
    let mem = std::fs::read_to_string(memory_path(root, &bot.id)).unwrap_or_default();
    p.push_str(&format!(
        "\n## Your memory\n\nYour notes file is `.viv/bots/{id}/memory.md` (the user can read and edit it). Add what you'll \
         want next time: decisions, preferences, where things are. Memory is a starting point, not the truth: before acting \
         on something you remember, re-check the current state.\n",
        id = bot.id
    ));
    if !mem.trim().is_empty() {
        let mem: String = mem.chars().take(6000).collect();
        p.push_str(&format!("\nWhat you've noted so far:\n\n{}\n", mem.trim()));
    }
    let r = &settings.rules;
    p.push_str(&format!(
        "\n## Your rules for acting\n\nChanging files: {:?}. Running commands: {:?}. Using the computer: {:?}. The web: {:?}. \
         Acting outside this machine (posting, sending, merging): {:?}. (Auto = go ahead; IfRequested = only when asked; \
         Ask = the user approves first; HandOff = don't, hand it to the user.)\n",
        r.files, r.commands, r.computer, r.web, r.outside
    ));
    p
}

fn first_text(messages: &[AgentMessage]) -> String {
    messages
        .iter()
        .rev()
        .find_map(|m| match m {
            AgentMessage::Assistant { content, .. } => {
                let t = content
                    .iter()
                    .filter_map(|c| if let MessageContent::Text { text } = c { Some(text.as_str()) } else { None })
                    .collect::<Vec<_>>()
                    .join("\n");
                (!t.trim().is_empty()).then(|| t.trim().to_string())
            }
            _ => None,
        })
        .unwrap_or_default()
}

fn room_digest(root: &Path, team: &[TeamAgent]) -> String {
    let name = |from: &str| -> String {
        if from == "user" {
            "User".into()
        } else {
            team.iter().find(|a| a.id == from).map(|a| format!("{} (@{})", a.name, a.id)).unwrap_or_else(|| from.to_string())
        }
    };
    read_lines::<ChatMessage>(&room_path(root), ROOM_IN_PROMPT)
        .iter()
        .map(|m| format!("[{}] {}", name(&m.from), m.text.chars().take(1500).collect::<String>()))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Keep the thread to its last messages, cut at a user turn so a tool
/// result never arrives without its call.
fn trim_thread(mut msgs: Vec<AgentMessage>) -> Vec<AgentMessage> {
    // Trim in one cut once the thread has grown well past the window, not one
    // message every turn: every change to the start of the history makes the
    // provider re-read the whole prompt (no cache hit), which on a long
    // thread is ~100k tokens of waiting before each reply.
    if msgs.len() <= THREAD_MAX {
        return msgs;
    }
    let mut start = msgs.len() - THREAD_KEPT;
    while start < msgs.len() && !matches!(msgs[start], AgentMessage::User { .. }) {
        start += 1;
    }
    msgs.drain(..start);
    msgs
}

/// Run one turn of a bot. Queues it if the bot is busy.
fn dispatch(app: &tauri::AppHandle, root: &str, id: &str, origin: Origin, request: String, target: Target) {
    dispatch_with(app, root, id, origin, request, target, Vec::new())
}

/// `dispatch`, with images for the agent to look at (a DM's attachments).
fn dispatch_with(app: &tauri::AppHandle, root: &str, id: &str, origin: Origin, request: String, target: Target, images: Vec<ImageInput>) {
    let key = (root.to_string(), id.to_string());
    {
        let mut l = live();
        if l.running.contains_key(&key) {
            l.queued.entry(key).or_default().push((origin, request, target, images));
            return;
        }
        l.running.insert(key.clone(), Arc::new(AtomicBool::new(false)));
        l.steps.remove(&key);
        l.channel.insert(key.clone(), if target == Target::Dm { id.to_string() } else { "room".to_string() });
    }
    emit_status(app, root, id);
    // Tells the chat where this run will show, before its first step.
    emit_steps(app, root, id);
    let app = app.clone();
    let (root, id) = key;
    // The loop's future isn't `Send` (provider streaming, event plumbing), so
    // like Viv Instant and subagents it runs start to finish on its own thread.
    std::thread::spawn(move || {
        let rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
            Ok(rt) => rt,
            Err(e) => {
                log::warn!("[bots] couldn't start a runtime: {e}");
                return;
            }
        };
        let outcome = rt.block_on(run_turn(&app, &root, &id, origin, &request, images));
        finish(&app, &root, &id, origin, &request, target, outcome);
    });
}

async fn run_turn(app: &tauri::AppHandle, root: &str, id: &str, origin: Origin, request: &str, images: Vec<ImageInput>) -> Result<String, String> {
    let root_path = PathBuf::from(root);
    let bot = find_bot(&root_path, id).ok_or_else(|| format!("@{id} isn't an active agent on this project"))?;
    let settings = load_settings(&root_path, id);
    if settings.paused && origin != Origin::User {
        return Err(format!("{} is paused", bot.name));
    }
    let team = active_bots(&root_path);
    let abort = live().running.get(&(root.to_string(), id.to_string())).cloned().unwrap_or_default();

    let registry = app.state::<crate::agent::runtime::commands::AgentRegistry>();
    let primary = registry.resolve(None).await;
    let state = crate::agent::runtime::state::AgentState::new();
    match bot.model.as_ref() {
        Some(m) => {
            state
                .set_model(crate::agent::runtime::types::ModelRef {
                    id: m.id.clone(),
                    name: m.name.clone(),
                    api: m.api.clone(),
                    provider: m.provider.clone(),
                    base_url: m.base_url.clone(),
                    reasoning: false,
                    max_tokens: m.max_tokens,
                    context_window: m.context_window,
                })
                .await
        }
        None => {
            // Routines and check-ins are background work: the quick model
            // when cost-aware routing is on (a no-op otherwise).
            let base = primary.agent_state.model().await;
            let model = if matches!(origin, Origin::Routine | Origin::Proactive) {
                // A routine's own text says how hard it is; with tiers set
                // that picks the model, else it is quick-model work.
                let (tiered, verdict) = crate::agent::models::routing::route_task(request, &base).await;
                if verdict.is_some() {
                    tiered
                } else {
                    crate::agent::models::routing::route(crate::agent::models::routing::Role::Routine, &base).await
                }
            } else {
                base
            };
            state.set_model(model).await
        }
    }
    // "Auto" isn't a model a request can go to: pick the tier for this
    // message, as the chat does for each of its turns. A bot set to Auto, or
    // one following a chat that is, gets Nano, Super or Ultra by difficulty.
    let picked = state.model().await;
    if crate::agent::models::routing::is_router(&picked) {
        let (model, _) = crate::agent::models::routing::route_task(request, &picked).await;
        state.set_model(model).await;
    }
    state.set_workspace(vec![root.to_string()], crate::agent::runtime::state::WorkspaceAccessMode::Full).await;
    state.set_persona_mode(PersonaMode::Agent).await;
    // Its own email and phone come with having them, whatever tools were picked.
    let contact = crate::agent::team::contact::load(root, id);
    let mut allowed = bot.allowed_tools.clone();
    if let Some(list) = allowed.as_mut() {
        if contact.email.is_some() && !list.iter().any(|t| t == "email") {
            list.push("email".into());
        }
        if contact.phone.is_some() && !list.iter().any(|t| t == "phone") {
            list.push("phone".into());
        }
    }
    state.set_subagent_allowed_tools(allowed).await;
    state.set_delegation_allowed(true).await;
    state
        .set_bot_policy(Some(BotPolicy {
            bot_name: bot.name.clone(),
            rules: settings.rules.clone(),
            origin,
            request: request.to_string(),
            root: root.to_string(),
            bot_id: id.to_string(),
        }))
        .await;

    let auth = app.state::<Arc<crate::ai::auth::AuthState>>().inner().clone();
    let blocks = crate::agent::runtime::directive::assemble_capability_blocks(&state, None, request, Some(&auth), true, false).await;
    let system_prompt = format!("{}\n\n{}", identity(&root_path, &bot, &settings, &team), blocks);
    state.set_system_prompt(system_prompt.clone()).await;

    let history: Vec<AgentMessage> = read_json(&thread_path(&root_path, id));
    let mut context = AgentContext { system_prompt, messages: history.clone(), ..Default::default() };

    let queues = crate::agent::runtime::loop_core::QueueHandles {
        steering_queue: Arc::new(tokio::sync::RwLock::new(Vec::new())),
        steering_mode: QueueMode::OneAtATime,
        follow_up_queue: Arc::new(tokio::sync::RwLock::new(Vec::new())),
        follow_up_mode: QueueMode::OneAtATime,
    };
    let steer = Arc::new(AtomicBool::new(false));

    // A private event bus, forwarded as bot activity and approval requests.
    let (tx, mut rx) = crate::agent::runtime::events::event_bus();
    let (app_ev, root_ev, id_ev) = (app.clone(), root.to_string(), id.to_string());
    let drain = tokio::spawn(async move {
        while let Some(event) = rx.rx.recv().await {
            match event {
                AgentEvent::ToolExecutionStart { tool_call_id, tool_name, args } => {
                    let label = step_label(&tool_name, &args);
                    {
                        let mut l = live();
                        let key = (root_ev.clone(), id_ev.clone());
                        l.activity.insert(key.clone(), label.clone());
                        let steps = l.steps.entry(key).or_default();
                        steps.push(BotStep { id: tool_call_id, tool: tool_name, label, state: "running".into(), detail: None });
                        if steps.len() > MAX_STEPS {
                            steps.remove(0);
                        }
                    }
                    emit_status(&app_ev, &root_ev, &id_ev);
                    emit_steps(&app_ev, &root_ev, &id_ev);
                }
                AgentEvent::ToolExecutionEnd { tool_call_id, result, is_error, .. } => {
                    {
                        let mut l = live();
                        if let Some(step) = l
                            .steps
                            .get_mut(&(root_ev.clone(), id_ev.clone()))
                            .and_then(|s| s.iter_mut().rev().find(|s| s.id == tool_call_id))
                        {
                            step.state = if is_error { "error" } else { "done" }.into();
                            let text = result_text(&result);
                            let first = text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
                            step.detail = (!first.is_empty()).then(|| clip(first, 160));
                        }
                    }
                    emit_steps(&app_ev, &root_ev, &id_ev);
                }
                AgentEvent::PermissionAsked { request_id, tool_name, args, reason, .. } => {
                    let _ = app_ev.emit(
                        "bots:permission",
                        serde_json::json!({ "root": root_ev, "botId": id_ev, "requestId": request_id, "toolName": tool_name, "args": args, "reason": reason }),
                    );
                    crate::agent::system::notifications::notify_text(&app_ev, "A teammate needs your approval", &format!("@{id_ev} wants to use {tool_name}"));
                }
                AgentEvent::PermissionResolved { request_id, .. } => {
                    let _ = app_ev.emit("bots:permission_resolved", serde_json::json!({ "requestId": request_id }));
                }
                _ => {}
            }
        }
    });

    let framed = match origin {
        Origin::User => request.to_string(),
        Origin::Routine => format!("(Routine) {request}"),
        Origin::Proactive | Origin::Inbound => request.to_string(),
    };
    // Images go in as content blocks, with a line saying they're there: some
    // models otherwise answer that they can't see images.
    let framed = if images.is_empty() {
        framed
    } else {
        format!("{framed}\n\n({} image{} attached to this message — look at it before answering.)", images.len(), if images.len() == 1 { "" } else { "s" })
    };
    let mut content = vec![MessageContent::Text { text: framed }];
    for img in images {
        content.push(MessageContent::Image { url: None, base64: Some(img.base64), media_type: Some(img.media_type) });
    }
    let new_messages = Box::pin(crate::agent::runtime::loop_core::run_agent_loop_with_prompts(
        vec![AgentMessage::User { content, timestamp: now_ms() }],
        app,
        &mut context,
        &state,
        &tx,
        &queues,
        &abort,
        &steer,
        "bot",
    ))
    .await;
    drop(tx);
    let _ = drain.await;
    crate::activity::end_run(app, "done", format!("{} finished", bot.name));

    let mut thread = history;
    thread.extend(new_messages.iter().cloned());
    let _ = write_json(&thread_path(&root_path, id), &trim_thread(thread));

    if abort.load(Ordering::SeqCst) {
        return Ok("Stopped.".into());
    }
    if let Some(AgentMessage::Assistant { stop_reason, error_message, .. }) =
        new_messages.iter().rev().find(|m| matches!(m, AgentMessage::Assistant { .. }))
    {
        if stop_reason == "error" {
            return Err(error_message.clone().unwrap_or_else(|| "the model call failed".into()));
        }
    }
    Ok(first_text(&new_messages))
}

fn mentions_in(text: &str, team: &[TeamAgent]) -> (Vec<String>, bool) {
    let lower = text.to_lowercase();
    let everyone = lower.contains("@everyone") || lower.contains("@all ") || lower.ends_with("@all");
    let ids = team
        .iter()
        .filter(|a| {
            let id = format!("@{}", a.id.to_lowercase());
            let name = format!("@{}", a.name.to_lowercase().replace(' ', ""));
            lower.contains(&id) || lower.replace(' ', "").contains(&name)
        })
        .map(|a| a.id.clone())
        .collect();
    (ids, everyone)
}

fn post(app: &tauri::AppHandle, root: &str, channel: &str, from: &str, text: &str, kind: &str, mentions: Vec<String>) -> ChatMessage {
    post_with_steps(app, root, channel, from, text, kind, mentions, Vec::new())
}

#[allow(clippy::too_many_arguments)]
fn post_with_steps(
    app: &tauri::AppHandle,
    root: &str,
    channel: &str,
    from: &str,
    text: &str,
    kind: &str,
    mentions: Vec<String>,
    steps: Vec<BotStep>,
) -> ChatMessage {
    let msg = ChatMessage {
        id: uuid::Uuid::new_v4().to_string(),
        from: from.into(),
        text: text.into(),
        ts: now_ms(),
        kind: kind.into(),
        mentions,
        steps,
        attachments: Vec::new(),
    };
    let path = if channel == "room" { room_path(Path::new(root)) } else { dm_path(Path::new(root), channel) };
    let _ = append_line(&path, &msg);
    emit_message(app, root, channel, &msg);
    msg
}

fn finish(app: &tauri::AppHandle, root: &str, id: &str, origin: Origin, request: &str, target: Target, outcome: Result<String, String>) {
    let root_path = PathBuf::from(root);
    let team = active_bots(&root_path);
    let name = team.iter().find(|a| a.id == id).map(|a| a.name.clone()).unwrap_or_else(|| id.to_string());
    // The run's steps go with what it posts, so they stay readable after.
    let steps = live().steps.remove(&(root.to_string(), id.to_string())).unwrap_or_default();

    match outcome {
        Ok(reply) => {
            let reply = reply.trim().to_string();
            let silent = origin == Origin::Proactive && (reply.is_empty() || reply.eq_ignore_ascii_case("nothing") || reply.starts_with("NOTHING"));
            if !silent && !reply.is_empty() {
                let (text, needs) = needs_you(&reply);
                let kind = if needs {
                    "needs_you"
                } else {
                    match origin {
                        Origin::Routine => "routine",
                        Origin::Proactive => "checkin",
                        Origin::User | Origin::Inbound => "message",
                    }
                };
                let channel = match target {
                    Target::Dm => id.to_string(),
                    Target::Room { .. } => "room".to_string(),
                };
                let (mentions, everyone) = mentions_in(&text, &team);
                post_with_steps(app, root, &channel, id, &text, kind, mentions.clone(), steps);
                if needs || origin != Origin::User {
                    let title = if needs { format!("{name} needs you") } else { format!("{name} posted") };
                    crate::agent::system::notifications::notify_text(app, &title, &text.chars().take(140).collect::<String>());
                }
                // Handoffs: teammates mentioned in a room post are asked next.
                if let Target::Room { hops } = target {
                    if hops < MAX_HOPS {
                        let next: Vec<String> =
                            if everyone { team.iter().map(|a| a.id.clone()).collect() } else { mentions };
                        for other in next.into_iter().filter(|o| o != id) {
                            dispatch(app, root, &other, Origin::User, format!("{name} in the room: {text}\n\nRecent room:\n{}", room_digest(&root_path, &team)), Target::Room { hops: hops + 1 });
                        }
                    }
                }
            }
        }
        Err(e) => {
            let channel = if target == Target::Dm { id.to_string() } else { "room".to_string() };
            // A provider's limit is something the user can fix in one click:
            // say so, rather than leaving a bare HTTP error.
            let low = e.to_lowercase();
            let hint = if low.contains("429") || low.contains("rate limit") || low.contains("quota") || low.contains("credits") || low.contains("insufficient") {
                " Its model is out of requests for now: pick another in the model menu below this chat (Auto uses your routing tiers)."
            } else {
                ""
            };
            post_with_steps(app, root, &channel, "system", &format!("{name} couldn't finish: {e}{hint}"), "note", Vec::new(), steps);
        }
    }
    if origin == Origin::Proactive {
        let mut s = load_settings(&root_path, id);
        s.last_checkin = Some(now_ms());
        let _ = save_settings(&root_path, id, &s);
    }
    let _ = request;

    // Next in line, if anything arrived while it worked.
    let key = (root.to_string(), id.to_string());
    let next = {
        let mut l = live();
        l.running.remove(&key);
        l.activity.remove(&key);
        l.steps.remove(&key);
        l.channel.remove(&key);
        l.queued.get_mut(&key).and_then(|q| (!q.is_empty()).then(|| q.remove(0)))
    };
    emit_status(app, root, id);
    if let Some((o, r, t, imgs)) = next {
        dispatch_with(app, root, id, o, r, t, imgs);
    }
}

/// Whether a reply asks the user for something, and the reply without the
/// marker. Models put "NEEDS YOU:" where the question is, often after an
/// explanation, so it counts at the start of any line, bold or not.
fn needs_you(reply: &str) -> (String, bool) {
    let mut found = false;
    let lines: Vec<String> = reply
        .lines()
        .map(|line| {
            let t = line.trim_start();
            for marker in ["**NEEDS YOU:**", "**NEEDS YOU**:", "NEEDS YOU:"] {
                if let Some(rest) = t.strip_prefix(marker) {
                    found = true;
                    return rest.trim_start().to_string();
                }
            }
            line.to_string()
        })
        .collect();
    (lines.join("
").trim().to_string(), found)
}

// ── Routing ──────────────────────────────────────────────────────────────

const ROUTER_PROMPT: &str = "You route a message in a team chat to the teammates who should answer it. \
Given the team (id: name — job) and the message, reply with ONLY a JSON array of the ids who should respond: \
usually one, the owner of the work; more only when the message clearly needs several. [] if nobody needs to.";

async fn route(app: &tauri::AppHandle, team: &[TeamAgent], text: &str) -> Vec<String> {
    if team.len() == 1 {
        return vec![team[0].id.clone()];
    }
    let registry = app.state::<crate::agent::runtime::commands::AgentRegistry>();
    let model = registry.resolve(None).await.agent_state.model().await;
    let roster = team.iter().map(|a| format!("{}: {} — {}", a.id, a.name, a.role)).collect::<Vec<_>>().join("\n");
    let prompt = format!("Team:\n{roster}\n\nMessage:\n{text}");
    let picked = crate::ai::completion::complete_background(app, &model, ROUTER_PROMPT, &prompt)
        .await
        .ok()
        .and_then(|r| crate::ai::completion::extract_json_array(&r))
        .and_then(|v| v.as_array().cloned())
        .unwrap_or_default();
    let ids: Vec<String> = picked
        .iter()
        .filter_map(|v| v.as_str())
        .filter(|id| team.iter().any(|a| a.id == *id))
        .map(String::from)
        .collect();
    // Routing failed or chose nobody for a direct question: the first
    // teammate answers rather than the message vanishing.
    if ids.is_empty() { vec![team[0].id.clone()] } else { ids }
}

// ── Routines and check-ins ───────────────────────────────────────────────

const CHECKIN_PROMPT: &str = "Check-in. Look at what has changed in the project and the room since your last check, \
and research what would help with your job. This is read-only: you can look, not change anything. If you found something \
worth the user's attention, post it briefly (and say what you'd do about it). If not, reply with exactly NOTHING.";

/// Ticks every 30 s while the app runs: due routines and check-ins.
pub fn spawn_scheduler(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(30)).await;
            for root in bot_roots() {
                tick(&app, &root);
            }
        }
    });
}

fn tick(app: &tauri::AppHandle, root: &str) {
    let root_path = PathBuf::from(root);
    let now = now_ms();
    for bot in active_bots(&root_path) {
        let settings = load_settings(&root_path, &bot.id);
        if settings.paused {
            continue;
        }
        let mut routines = load_routines(&root_path, &bot.id);
        let mut changed = false;
        for r in routines.iter_mut().filter(|r| r.enabled) {
            match r.next_run {
                Some(at) if at <= now => {
                    r.next_run = next_run(&r.cron);
                    changed = true;
                    let request = format!("{}\n\n{}", r.title, r.prompt);
                    dispatch(app, root, &bot.id, Origin::Routine, request, Target::Room { hops: MAX_HOPS });
                    r.runs.push(RoutineRun { at: now, ok: true, summary: "Started".into() });
                    let skip = r.runs.len().saturating_sub(RUNS_KEPT);
                    r.runs.drain(..skip);
                }
                None => {
                    r.next_run = next_run(&r.cron);
                    changed = true;
                }
                _ => {}
            }
        }
        if changed {
            let _ = save_routines(&root_path, &bot.id, &routines);
        }
        if settings.always_on {
            let every = u64::from(settings.checkin_hours.max(1)) * 3_600_000;
            if settings.last_checkin.map_or(true, |t| now.saturating_sub(t) >= every) {
                let mut s = settings.clone();
                s.last_checkin = Some(now);
                let _ = save_settings(&root_path, &bot.id, &s);
                let request = format!("{CHECKIN_PROMPT}\n\nRecent room:\n{}", room_digest(&root_path, &active_bots(&root_path)));
                dispatch(app, root, &bot.id, Origin::Proactive, request, Target::Room { hops: MAX_HOPS });
            }
        }
    }
}

// ── Commands ─────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn cmd_bots_room(root: String, limit: Option<usize>) -> Result<Vec<ChatMessage>, String> {
    Ok(read_lines(&room_path(Path::new(&root)), limit.unwrap_or(200)))
}

/// The user posts in the room: mentioned bots answer, or whoever routing picks.
#[tauri::command]
pub async fn cmd_bots_post(app: tauri::AppHandle, root: String, text: String) -> Result<ChatMessage, String> {
    let text = text.trim().to_string();
    if text.is_empty() {
        return Err("Nothing to send.".into());
    }
    let root_path = PathBuf::from(&root);
    let team = active_bots(&root_path);
    if team.is_empty() {
        return Err("There are no agents on this team yet — add one first.".into());
    }
    let (mut mentions, everyone) = mentions_in(&text, &team);
    let msg = post(&app, &root, "room", "user", &text, "message", mentions.clone());
    if everyone {
        mentions = team.iter().map(|a| a.id.clone()).collect();
    } else if mentions.is_empty() {
        mentions = route(&app, &team, &text).await;
    }
    let digest = room_digest(&root_path, &team);
    for id in mentions {
        let request = format!("The user in the room: {text}\n\nRecent room:\n{digest}");
        dispatch(&app, &root, &id, Origin::User, request, Target::Room { hops: 0 });
    }
    Ok(msg)
}

#[tauri::command]
pub async fn cmd_bots_dm(root: String, bot_id: String, limit: Option<usize>) -> Result<Vec<ChatMessage>, String> {
    Ok(read_lines(&dm_path(Path::new(&root), &bot_id), limit.unwrap_or(200)))
}

/// A bot's whole working transcript: its messages with every tool call and
/// result, the same shape the main chat renders. The DM view shows this so
/// an agent's work reads like the coding agent's, not as a folded step list.
#[tauri::command]
pub async fn cmd_bots_thread(root: String, bot_id: String) -> Result<Vec<AgentMessage>, String> {
    Ok(read_json(&thread_path(Path::new(&root), &bot_id)))
}

/// What a revert did: how many messages went, and which files came back.
#[derive(Debug, Clone, Serialize)]
pub struct BotRevert {
    pub removed: usize,
    pub restored: Vec<String>,
    pub deleted: Vec<String>,
    pub failed: Vec<String>,
}

/// Go back to just before message `index` of a bot's transcript: the messages
/// from there on are dropped, and every file the bot changed in them is put
/// back as it was (newest change first), from the checkpoints taken before
/// each edit. A bot that is working can't be reverted.
#[tauri::command]
pub async fn cmd_bots_revert(root: String, bot_id: String, index: usize) -> Result<BotRevert, String> {
    let root_path = Path::new(&root);
    if status_of(&root, &bot_id).state == "working" {
        return Err("It's still working: stop it first, then go back.".into());
    }
    let path = thread_path(root_path, &bot_id);
    let mut thread: Vec<AgentMessage> = read_json(&path);
    if index >= thread.len() {
        return Ok(BotRevert { removed: 0, restored: Vec::new(), deleted: Vec::new(), failed: Vec::new() });
    }
    let dropped: std::collections::HashSet<String> = thread[index..]
        .iter()
        .filter_map(|m| match m {
            AgentMessage::Assistant { content, .. } => Some(content),
            _ => None,
        })
        .flatten()
        .filter_map(|c| match c {
            MessageContent::ToolCall(tc) => Some(tc.id.clone()),
            _ => None,
        })
        .collect();
    let session = crate::agent::changes::checkpoints::bot_session(&root, &bot_id);
    let mut out = BotRevert { removed: thread.len() - index, restored: Vec::new(), deleted: Vec::new(), failed: Vec::new() };
    // `list` is newest first: undoing in that order leaves each file as it was
    // before the earliest dropped edit.
    for cp in crate::agent::changes::checkpoints::list(&session).into_iter().filter(|c| dropped.contains(&c.tool_call_id)) {
        match crate::agent::changes::checkpoints::restore(&session, &cp.id) {
            Ok(r) => {
                out.restored.extend(r.restored);
                out.deleted.extend(r.deleted);
                out.failed.extend(r.failed);
            }
            Err(e) => out.failed.push(e),
        }
    }
    out.restored.sort();
    out.restored.dedup();
    out.deleted.sort();
    out.deleted.dedup();
    thread.truncate(index);
    write_json(&path, &thread)?;
    Ok(out)
}

#[tauri::command]
pub async fn cmd_bots_dm_send(
    app: tauri::AppHandle,
    root: String,
    bot_id: String,
    text: String,
    files: Option<Vec<FileInput>>,
    images: Option<Vec<ImageInput>>,
) -> Result<ChatMessage, String> {
    let text = text.trim().to_string();
    let files = files.unwrap_or_default();
    let images = images.unwrap_or_default();
    if text.is_empty() && files.is_empty() && images.is_empty() {
        return Err("Nothing to send.".into());
    }
    find_bot(Path::new(&root), &bot_id).ok_or("That agent isn't active.")?;
    // The chat shows what was written and chips for what was attached; the
    // agent gets the files' contents inline, as the main chat does.
    let mut attachments: Vec<AttachmentRef> = files.iter().map(|f| AttachmentRef { name: f.name.clone(), kind: "file".into() }).collect();
    attachments.extend((1..=images.len()).map(|i| AttachmentRef { name: format!("Image {i}"), kind: "image".into() }));
    let shown = if text.is_empty() { "(attachments)".to_string() } else { text.clone() };
    let msg = ChatMessage {
        id: uuid::Uuid::new_v4().to_string(),
        from: "user".into(),
        text: shown,
        ts: now_ms(),
        kind: "message".into(),
        mentions: Vec::new(),
        steps: Vec::new(),
        attachments,
    };
    let _ = append_line(&dm_path(Path::new(&root), &bot_id), &msg);
    emit_message(&app, &root, &bot_id, &msg);
    let request = if files.is_empty() {
        text
    } else {
        let blocks = files
            .iter()
            .map(|f| format!("<file path=\"{}\"{}>\n{}\n</file>", f.name, if f.truncated { " truncated=\"true\"" } else { "" }, f.content))
            .collect::<Vec<_>>()
            .join("\n\n");
        format!("{text}\n\nAttached files:\n{blocks}")
    };
    dispatch_with(&app, &root, &bot_id, Origin::User, request, Target::Dm, images);

    Ok(msg)
}

#[tauri::command]
pub async fn cmd_bots_status(root: String) -> Result<Vec<BotStatus>, String> {
    Ok(active_bots(Path::new(&root)).iter().map(|a| status_of(&root, &a.id)).collect())
}

#[tauri::command]
pub async fn cmd_bots_settings(root: String, bot_id: String) -> Result<BotSettings, String> {
    Ok(load_settings(Path::new(&root), &bot_id))
}

#[tauri::command]
pub async fn cmd_bots_save_settings(app: tauri::AppHandle, root: String, bot_id: String, settings: BotSettings) -> Result<BotSettings, String> {
    let mut s = settings;
    s.checkin_hours = s.checkin_hours.clamp(1, 168);
    s.last_checkin = load_settings(Path::new(&root), &bot_id).last_checkin;
    save_settings(Path::new(&root), &bot_id, &s)?;
    emit_status(&app, &root, &bot_id);
    Ok(s)
}

/// Stop what it's doing now. Already-finished actions are not undone.
#[tauri::command]
pub async fn cmd_bots_stop(app: tauri::AppHandle, root: String, bot_id: String) -> Result<(), String> {
    let key = (root.clone(), bot_id.clone());
    let flag = {
        let mut l = live();
        l.queued.remove(&key);
        l.running.get(&key).cloned()
    };
    if let Some(f) = flag {
        f.store(true, Ordering::SeqCst);
        crate::coding_agent::win_cancel();
    }
    emit_status(&app, &root, &bot_id);
    Ok(())
}

/// Forget everything: conversation, DMs, memory, routines' history. The
/// definition stays.
#[tauri::command]
pub async fn cmd_bots_reset(app: tauri::AppHandle, root: String, bot_id: String) -> Result<(), String> {
    cmd_bots_stop(app.clone(), root.clone(), bot_id.clone()).await?;
    let dir = bot_dir(Path::new(&root), &bot_id);
    for f in ["thread.json", "dm.jsonl", "memory.md"] {
        let _ = std::fs::remove_file(dir.join(f));
    }
    let mut routines = load_routines(Path::new(&root), &bot_id);
    for r in routines.iter_mut() {
        r.runs.clear();
    }
    save_routines(Path::new(&root), &bot_id, &routines)?;
    emit_status(&app, &root, &bot_id);
    Ok(())
}

#[tauri::command]
pub async fn cmd_bots_memory(root: String, bot_id: String) -> Result<String, String> {
    Ok(std::fs::read_to_string(memory_path(Path::new(&root), &bot_id)).unwrap_or_default())
}

#[tauri::command]
pub async fn cmd_bots_save_memory(root: String, bot_id: String, text: String) -> Result<(), String> {
    let path = memory_path(Path::new(&root), &bot_id);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(path, text).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn cmd_bots_routines(root: String, bot_id: String) -> Result<Vec<Routine>, String> {
    Ok(load_routines(Path::new(&root), &bot_id))
}

#[tauri::command]
pub async fn cmd_bots_save_routine(root: String, bot_id: String, routine: Routine) -> Result<Vec<Routine>, String> {
    let root_path = Path::new(&root);
    let mut r = routine;
    if r.title.trim().is_empty() || r.prompt.trim().is_empty() {
        return Err("A routine needs a title and what to do.".into());
    }
    r.next_run = Some(next_run(&r.cron).ok_or_else(|| format!("'{}' isn't a schedule I understand (use cron, e.g. 0 9 * * 1-5).", r.cron))?);
    let mut all = load_routines(root_path, &bot_id);
    if r.id.is_empty() {
        if all.len() >= MAX_ROUTINES {
            return Err(format!("A bot can have up to {MAX_ROUTINES} routines."));
        }
        r.id = uuid::Uuid::new_v4().to_string();
        all.push(r);
    } else if let Some(existing) = all.iter_mut().find(|x| x.id == r.id) {
        r.runs = std::mem::take(&mut existing.runs);
        *existing = r;
    } else {
        return Err("That routine no longer exists.".into());
    }
    save_routines(root_path, &bot_id, &all)?;
    Ok(all)
}

#[tauri::command]
pub async fn cmd_bots_delete_routine(root: String, bot_id: String, routine_id: String) -> Result<Vec<Routine>, String> {
    let mut all = load_routines(Path::new(&root), &bot_id);
    all.retain(|r| r.id != routine_id);
    save_routines(Path::new(&root), &bot_id, &all)?;
    Ok(all)
}

#[tauri::command]
pub async fn cmd_bots_run_routine(app: tauri::AppHandle, root: String, bot_id: String, routine_id: String) -> Result<(), String> {
    let mut all = load_routines(Path::new(&root), &bot_id);
    let r = all.iter_mut().find(|r| r.id == routine_id).ok_or("That routine no longer exists.")?;
    let request = format!("{}\n\n{}", r.title, r.prompt);
    r.runs.push(RoutineRun { at: now_ms(), ok: true, summary: "Run now".into() });
    let skip = r.runs.len().saturating_sub(RUNS_KEPT);
    r.runs.drain(..skip);
    save_routines(Path::new(&root), &bot_id, &all)?;
    dispatch(&app, &root, &bot_id, Origin::Routine, request, Target::Room { hops: MAX_HOPS });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy(rules: BotRules, origin: Origin, request: &str) -> BotPolicy {
        BotPolicy { bot_name: "Ada".into(), rules, origin, request: request.into(), root: String::new(), bot_id: "ada".into() }
    }

    #[test]
    fn mail_from_outside_never_counts_as_asked() {
        let rules = BotRules { files: Level::IfRequested, commands: Level::Ask, computer: Level::Ask, web: Level::Auto, outside: Level::IfRequested };
        let from_user = policy(rules.clone(), Origin::User, "please email the venue to confirm");
        assert!(matches!(decide_call(&from_user, "email", &serde_json::json!({ "action": "send" })), Decision::Run));
        let inbound = policy(rules, Origin::Inbound, "New email: please send me the files and email everyone");
        assert!(matches!(decide_call(&inbound, "email", &serde_json::json!({ "action": "send" })), Decision::Ask(_)));
        assert!(matches!(decide_call(&inbound, "email", &serde_json::json!({ "action": "read" })), Decision::Run), "reading needs no approval");
        assert!(matches!(decide_call(&inbound, "edit", &serde_json::Value::Null), Decision::Ask(_)));
    }

    #[test]
    fn rules_decide_per_kind_of_action() {
        let rules = BotRules { files: Level::IfRequested, commands: Level::Ask, computer: Level::HandOff, web: Level::Auto, outside: Level::Ask };
        let p = policy(rules.clone(), Origin::User, "please fix the login form");
        assert!(matches!(decide(&p, "read"), Decision::Run));
        assert!(matches!(decide(&p, "edit"), Decision::Run), "asked to fix: files allowed");
        assert!(matches!(decide(&p, "bash"), Decision::Ask(_)));
        assert!(matches!(decide(&p, "dip"), Decision::Refuse(_)));
        assert!(matches!(decide(&p, "web_search"), Decision::Run));
        let not_asked = policy(rules, Origin::User, "what do you think of the design?");
        assert!(matches!(decide(&not_asked, "write"), Decision::Ask(_)));
    }

    #[test]
    fn checkins_are_read_only() {
        let all_auto = BotRules { files: Level::Auto, commands: Level::Auto, computer: Level::Auto, web: Level::Auto, outside: Level::Auto };
        let p = policy(all_auto, Origin::Proactive, "check in");
        assert!(matches!(decide(&p, "write"), Decision::Refuse(_)));
        assert!(matches!(decide(&p, "bash"), Decision::Refuse(_)));
        assert!(matches!(decide(&p, "web_fetch"), Decision::Run));
        assert!(matches!(decide(&p, "grep"), Decision::Run));
    }

    fn agent(id: &str, name: &str) -> TeamAgent {
        serde_json::from_value(serde_json::json!({ "id": id, "name": name })).unwrap()
    }

    #[test]
    fn mentions_find_teammates_by_id_or_name() {
        let team = vec![agent("ada", "Ada Lovelace"), agent("lin", "Lin")];
        assert_eq!(mentions_in("hey @ada can you look", &team), (vec!["ada".to_string()], false));
        assert_eq!(mentions_in("@AdaLovelace and @lin", &team).0.len(), 2);
        assert!(mentions_in("@everyone standup", &team).1);
        assert!(mentions_in("no mentions here", &team).0.is_empty());
    }

    #[test]
    fn needs_you_is_found_on_any_line() {
        let (text, needs) = needs_you("Nothing to hand over yet.

NEEDS YOU: what are we building?");
        assert!(needs);
        assert_eq!(text, "Nothing to hand over yet.

what are we building?");
        assert!(needs_you("**NEEDS YOU:** pick a stack").1);
        assert!(!needs_you("All done, nothing needed from you.").1);
    }

    #[test]
    fn thread_trims_at_a_user_turn() {
        let user = || AgentMessage::User { content: vec![], timestamp: 0 };
        let tool = || AgentMessage::ToolResult {
            tool_call_id: "t".into(),
            tool_name: "read".into(),
            content: vec![],
            details: None,
            is_error: None,
            timestamp: 0,
            arguments: None,
        };
        // Under the cap nothing moves, so the history's start (and the
        // provider's prompt cache) stays put from turn to turn.
        let short: Vec<_> = (0..THREAD_MAX).map(|i| if i % 3 == 0 { user() } else { tool() }).collect();
        assert_eq!(trim_thread(short).len(), THREAD_MAX);
        // Past it, one cut back to the window, starting at a user turn.
        let mut msgs = vec![tool(); 10];
        msgs.extend((0..THREAD_MAX).map(|i| if i % 3 == 0 { user() } else { tool() }));
        let kept = trim_thread(msgs);
        assert!(kept.len() <= THREAD_KEPT);
        assert!(matches!(kept[0], AgentMessage::User { .. }));
    }
}

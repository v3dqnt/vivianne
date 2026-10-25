//! `app run`: one agent task from the command line, no window and no
//! webview, for benchmarks and scripts.
//!
//! ```text
//! app run "fix the failing test" --cwd /repo --model nebius/<model-id> --out result.json
//! ```
//!
//! The same agent loop, tools and permissions run as in the app. Two things
//! differ: model calls go through the Rust streaming code instead of the
//! webview's providers (`native()`), and every tool is approved (there is no
//! one to ask), so run it only on a folder you are happy to let it change.
//!
//! Output: progress lines on stderr, and (with `--out`) a JSON file with the
//! final answer, the transcript, the time taken and an exit status:
//! 0 finished, 1 the agent reported an error, 2 bad arguments, 3 ran out of
//! time.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use tauri::{Listener, Manager};

use crate::agent::runtime::commands::{cmd_agent_prompt, AgentRegistry};
use crate::agent::runtime::state::WorkspaceAccessMode;
use crate::agent::runtime::types::{AgentMessage, MessageContent, ModelRef, PersonaMode, ThinkingLevel};

static NATIVE: AtomicBool = AtomicBool::new(false);

/// Whether model calls are made by the Rust streaming code (CLI runs) rather
/// than asked of the webview's providers (the app).
pub fn native() -> bool {
    NATIVE.load(Ordering::Relaxed)
}

/// The parsed command line of an `app run ...` invocation.
#[derive(Debug, Clone, PartialEq)]
pub struct RunArgs {
    pub task: String,
    pub cwd: String,
    /// `provider/model-id`
    pub model: Option<String>,
    pub thinking: Option<String>,
    pub max_minutes: u64,
    pub out: Option<String>,
    /// `chat`: a conversation, one line at a time, instead of a single task.
    pub interactive: bool,
}

/// Parse `run <task> [--cwd DIR] [--model P/ID] [--thinking L] [--max-minutes N] [--out FILE]`
/// from the program's arguments (without the program name). `None` when this
/// is not a `run` invocation.
pub fn parse(args: &[String]) -> Option<Result<RunArgs, String>> {
    let interactive = match args.first().map(String::as_str) {
        Some("run") => false,
        Some("chat") => true,
        _ => return None,
    };
    let mut task: Option<String> = None;
    let mut cwd = std::env::current_dir().map(|p| p.display().to_string()).unwrap_or_else(|_| ".".into());
    let (mut model, mut thinking, mut out) = (None, None, None);
    let mut max_minutes = 30u64;
    let mut it = args[1..].iter();
    while let Some(a) = it.next() {
        let mut value = |flag: &str| it.next().cloned().ok_or_else(|| format!("{flag} needs a value"));
        let r: Result<(), String> = (|| {
            match a.as_str() {
                "--cwd" => cwd = value("--cwd")?,
                "--model" => model = Some(value("--model")?),
                "--thinking" => thinking = Some(value("--thinking")?),
                "--out" => out = Some(value("--out")?),
                "--max-minutes" => {
                    max_minutes = value("--max-minutes")?.parse().map_err(|_| "--max-minutes must be a whole number".to_string())?
                }
                // Allow the task to be given as the first free word(s).
                s if !s.starts_with("--") => {
                    task = Some(match task.take() {
                        Some(t) => format!("{t} {s}"),
                        None => s.to_string(),
                    })
                }
                other => return Err(format!("unknown option {other}")),
            }
            Ok(())
        })();
        if let Err(e) = r {
            return Some(Err(e));
        }
    }
    let task = task.unwrap_or_default();
    if !interactive && task.trim().is_empty() {
        return Some(Err("usage: app run \"<task>\" [--cwd DIR] [--model auto|provider/id] [--thinking level] [--max-minutes N] [--out FILE]".into()));
    }
    if max_minutes == 0 || max_minutes > 24 * 60 {
        return Some(Err("--max-minutes must be between 1 and 1440".into()));
    }
    Some(Ok(RunArgs { task, cwd, model, thinking, max_minutes, out, interactive }))
}

/// Providers `app run` can call, with the key each reads from the
/// environment when none is saved in the app.
pub const PROVIDERS: &[(&str, &str)] = &[
    ("anthropic", "ANTHROPIC_API_KEY"),
    ("openai", "OPENAI_API_KEY"),
    ("google", "GEMINI_API_KEY"),
    ("mistral", "MISTRAL_API_KEY"),
    ("openrouter", "OPENROUTER_API_KEY"),
    ("nebius", "NEBIUS_API_KEY"),
    ("nvidia-nim", "NVIDIA_NIM_API_KEY"),
    ("groq", "GROQ_API_KEY"),
    ("xai", "XAI_API_KEY"),
    ("deepseek", "DEEPSEEK_API_KEY"),
    ("together", "TOGETHER_API_KEY"),
    ("fireworks", "FIREWORKS_API_KEY"),
    ("cerebras", "CEREBRAS_API_KEY"),
    ("huggingface", "HF_TOKEN"),
    ("cloudflare-workers-ai", "CLOUDFLARE_API_KEY"),
    ("bedrock", "AWS_BEARER_TOKEN_BEDROCK or AWS credentials"),
    ("azure-openai-responses", "AZURE_OPENAI_API_KEY"),
    ("local", "(a running llama.cpp server)"),
];

/// `app run --help`.
pub fn help() -> String {
    let mut s = String::from(
        "app run: run one agent task from the command line, with no window

         usage: app run \"<task>\" [options]

         options:
             --cwd DIR            folder to work in (default: the current one)
             --model M            auto (the cost-aware router) or provider/model-id
             --thinking LEVEL     off, minimal, low, medium, high, max
             --max-minutes N      time limit, 1-1440 (default 30)
             --out FILE           write the answer, transcript, steps, seconds and tokens as JSON

         every tool is approved automatically: run it only on a folder you are happy to let it change.
         exit status: 0 done, 1 error, 2 bad arguments, 3 out of time

         providers (key from the app's saved keys, else from the environment):
",
    );
    for (name, key) in PROVIDERS {
        s.push_str(&format!("  {name:<24} {key}
"));
    }
    s
}

/// Whether the arguments ask for help.
pub fn wants_help(args: &[String]) -> bool {
    matches!(args.first().map(String::as_str), Some("run") | Some("chat")) && args.iter().any(|a| a == "--help" || a == "-h")
}

/// The program's own arguments, after the executable.
pub fn program_args() -> Vec<String> {
    std::env::args().skip(1).collect()
}

/// Whether this process was started as `app run ...`.
pub fn is_run() -> bool {
    matches!(program_args().first().map(String::as_str), Some("run") | Some("chat"))
}

/// Turn on the Rust model path. Called once, before the app starts.
pub fn enable_native() {
    NATIVE.store(true, Ordering::Relaxed);
}

fn thinking_from(level: &str) -> ThinkingLevel {
    match level.to_lowercase().as_str() {
        "minimal" => ThinkingLevel::Minimal,
        "low" => ThinkingLevel::Low,
        "medium" => ThinkingLevel::Medium,
        "high" => ThinkingLevel::High,
        "max" | "xhigh" => ThinkingLevel::XHigh,
        _ => ThinkingLevel::Off,
    }
}

/// A model reference from `provider/id`, with the wire format and base URL
/// the provider registry says that provider uses.
fn model_from(spec: &str) -> Result<ModelRef, String> {
    // The cost-aware router: each task is graded and sent to the Easy,
    // Medium or Hard model set in Settings (the same as picking "Auto").
    if matches!(spec, "auto" | "router" | "router/viv-auto") {
        return Ok(ModelRef {
            id: crate::agent::models::routing::ROUTER_ID.to_string(),
            name: "Auto (cost-aware router)".to_string(),
            api: "openai-completions".to_string(),
            provider: crate::agent::models::routing::ROUTER_PROVIDER.to_string(),
            base_url: None,
            reasoning: false,
            max_tokens: 16_384,
            context_window: 128_000,
        });
    }
    let (provider, id) = spec.split_once('/').ok_or("--model must look like provider/model-id, e.g. nebius/<model>, or auto")?;
    if provider == "chatgpt" {
        return Err("the ChatGPT sign-in provider is not available in app run: use openai with an API key".into());
    }
    let ps = crate::ai::provider_registry::provider_spec(provider).ok_or_else(|| format!("unknown provider \"{provider}\""))?;
    use crate::ai::provider_registry::ApiKind;
    let api = match ps.api_kind {
        ApiKind::AnthropicMessages => "anthropic-messages",
        ApiKind::OpenAiChat => "openai-completions",
        ApiKind::OpenAiResponses => "openai-responses",
        ApiKind::GoogleGenerative => "google-generative-ai",
        ApiKind::MistralChat => "mistral-conversations",
        ApiKind::Bedrock => "bedrock-converse-stream",
        ApiKind::AzureOpenAiResponses => "azure-openai-responses",
    };
    Ok(ModelRef {
        id: id.to_string(),
        name: id.to_string(),
        api: api.to_string(),
        provider: provider.to_string(),
        base_url: ps.default_base_url.map(String::from),
        reasoning: false,
        max_tokens: 16_384,
        context_window: 128_000,
    })
}

fn finish(code: i32, args: &RunArgs, body: serde_json::Value) -> ! {
    if let Some(path) = &args.out {
        if let Err(e) = std::fs::write(path, serde_json::to_vec_pretty(&body).unwrap_or_default()) {
            eprintln!("[run] could not write {path}: {e}");
        }
    }
    std::process::exit(code)
}

/// Run the task and exit the process with its status. Never returns.
pub async fn run(app: tauri::AppHandle, args: RunArgs) -> ! {
    let started = Instant::now();
    let fail = |code: i32, error: String| -> ! {
        eprintln!("[run] {error}");
        finish(code, &args, serde_json::json!({ "ok": false, "error": error, "seconds": started.elapsed().as_secs_f64() }))
    };

    let cwd = match std::path::Path::new(&args.cwd).canonicalize() {
        Ok(p) => p.display().to_string().trim_start_matches(r"\\?\").to_string(),
        Err(_) => fail(2, format!("--cwd {} is not a folder", args.cwd)),
    };
    let model = match args.model.as_deref().map(model_from).transpose() {
        Ok(m) => m,
        Err(e) => fail(2, e),
    };

    let registry = app.state::<AgentRegistry>();
    let state = registry.resolve(Some("cli-run")).await;
    if let Some(m) = model {
        state.agent_state.set_model(m).await;
    }
    if let Some(level) = &args.thinking {
        state.agent_state.set_thinking_level(thinking_from(level)).await;
    }
    state.agent_state.set_workspace(vec![cwd.clone()], WorkspaceAccessMode::Full).await;
    state.agent_state.set_persona_mode(PersonaMode::Agent).await;
    // Nobody is there to approve a tool call.
    state.agent_state.set_auto_approve(true).await;

    // Print what the agent does, as it does it.
    let steps = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counter = steps.clone();
    app.listen("agent_event", move |e| {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(e.payload()) else { return };
        if v["threadId"] != "cli-run" {
            return;
        }
        if v["event"]["type"] == "tool_execution_start" {
            let n = counter.fetch_add(1, Ordering::Relaxed) + 1;
            let name = v["event"]["payload"]["tool_name"].as_str().unwrap_or("tool");
            eprintln!("[run] step {n}: {name}");
        }
    });

    eprintln!("[run] working in {cwd}");
    if args.interactive {
        chat_loop(&app, &state, &args, &cwd).await;
    }
    let prompt = cmd_agent_prompt(args.task.clone(), None, Some("cli-run".into()), app.clone(), registry);
    let outcome = tokio::time::timeout(Duration::from_secs(args.max_minutes * 60), prompt).await;
    let seconds = started.elapsed().as_secs_f64();
    match outcome {
        Err(_) => {
            state.abort_flag.store(true, Ordering::SeqCst);
            finish(3, &args, serde_json::json!({ "ok": false, "error": "out of time", "seconds": seconds, "steps": steps.load(Ordering::Relaxed) }))
        }
        Ok(Err(e)) => fail(1, e),
        Ok(Ok(_)) => {
            // The turn's messages are in the agent's state: the prompt call
            // itself returns none.
            let messages = state.agent_state.messages().await;
            // A model call the provider refused ends the turn quietly with an
            // error on the last assistant message: that is a failed run, not
            // a finished one.
            if let Some(AgentMessage::Assistant { error_message, stop_reason, .. }) =
                messages.iter().rev().find(|m| matches!(m, AgentMessage::Assistant { .. }))
            {
                if error_message.is_some() || stop_reason == "error" {
                    let why = error_message.clone().unwrap_or_else(|| "the model call failed".into());
                    let why: String = why.chars().take(600).collect();
                    eprintln!("[run] the model call failed: {why}");
                    finish(
                        1,
                        &args,
                        serde_json::json!({ "ok": false, "error": why, "seconds": seconds, "steps": steps.load(Ordering::Relaxed), "model": args.model, "messages": messages }),
                    );
                }
            }
            let (mut input, mut output, mut cache_read) = (0u64, 0u64, 0u64);
            for m in &messages {
                if let AgentMessage::Assistant { usage: Some(u), .. } = m {
                    input += u64::from(u.input);
                    output += u64::from(u.output);
                    cache_read += u64::from(u.cache_read);
                }
            }
            let answer = messages
                .iter()
                .rev()
                .find_map(|m| match m {
                    AgentMessage::Assistant { content, .. } => Some(
                        content
                            .iter()
                            .filter_map(|c| match c {
                                MessageContent::Text { text } => Some(text.as_str()),
                                _ => None,
                            })
                            .collect::<Vec<_>>()
                            .join("
"),
                    ),
                    _ => None,
                })
                // The last message may be a bare tool call: the answer is the
                // last assistant message that said something.
                .filter(|t| !t.trim().is_empty())
                .or_else(|| {
                    messages.iter().rev().find_map(|m| match m {
                        AgentMessage::Assistant { content, .. } => {
                            let t = content
                                .iter()
                                .filter_map(|c| match c {
                                    MessageContent::Text { text } => Some(text.as_str()),
                                    _ => None,
                                })
                                .collect::<Vec<_>>()
                                .join("
");
                            (!t.trim().is_empty()).then_some(t)
                        }
                        _ => None,
                    })
                })
                .unwrap_or_default();
            eprintln!("[run] done in {seconds:.1}s");
            finish(
                0,
                &args,
                serde_json::json!({
                    "ok": true,
                    "seconds": seconds,
                    "steps": steps.load(Ordering::Relaxed),
                    "model": args.model,
                    "answer": answer,
                    "tokens": { "input": input, "output": output, "cacheRead": cache_read },
                    "messages": messages,
                }),
            )
        }
    }
}

/// The last thing the assistant said in the turn just finished: the last
/// assistant message with text, after the last user message.
fn turn_answer(messages: &[AgentMessage]) -> String {
    let since = messages.iter().rposition(|m| matches!(m, AgentMessage::User { .. })).map(|i| i + 1).unwrap_or(0);
    messages[since..]
        .iter()
        .rev()
        .find_map(|m| match m {
            AgentMessage::Assistant { content, .. } => {
                let t = content
                    .iter()
                    .filter_map(|c| match c {
                        MessageContent::Text { text } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("
");
                (!t.trim().is_empty()).then_some(t)
            }
            _ => None,
        })
        .unwrap_or_default()
}

/// `app chat`: read a line, run it as a turn on the same conversation, print
/// the answer; `/exit` (or the end of input) leaves.
async fn chat_loop(app: &tauri::AppHandle, state: &std::sync::Arc<crate::agent::runtime::commands::AppState>, args: &RunArgs, cwd: &str) -> ! {
    use tokio::io::AsyncBufReadExt;
    eprintln!("[chat] in {cwd}. Every tool is approved automatically. Type /exit to leave.");
    let mut lines = tokio::io::BufReader::new(tokio::io::stdin()).lines();
    let mut next = if args.task.trim().is_empty() { None } else { Some(args.task.clone()) };
    loop {
        let line = match next.take() {
            Some(l) => l,
            None => {
                eprint!("> ");
                match lines.next_line().await {
                    Ok(Some(l)) => l,
                    _ => break,
                }
            }
        };
        let line = line.trim().to_string();
        if line.is_empty() {
            continue;
        }
        if matches!(line.as_str(), "/exit" | "/quit") {
            break;
        }
        let registry = app.state::<AgentRegistry>();
        let turn = cmd_agent_prompt(line, None, Some("cli-run".into()), app.clone(), registry);
        match tokio::time::timeout(Duration::from_secs(args.max_minutes * 60), turn).await {
            Err(_) => {
                state.abort_flag.store(true, Ordering::SeqCst);
                eprintln!("[chat] that took longer than {} minutes and was stopped", args.max_minutes);
            }
            Ok(Err(e)) => eprintln!("[chat] {e}"),
            Ok(Ok(_)) => {
                let messages = state.agent_state.messages().await;
                if let Some(AgentMessage::Assistant { error_message: Some(e), .. }) = messages.iter().rev().find(|m| matches!(m, AgentMessage::Assistant { .. })) {
                    eprintln!("[chat] the model call failed: {}", e.chars().take(400).collect::<String>());
                } else {
                    println!("{}", turn_answer(&messages));
                }
            }
        }
    }
    std::process::exit(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(s: &[&str]) -> Vec<String> {
        s.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn only_run_is_a_cli_invocation() {
        assert!(parse(&a(&[])).is_none());
        assert!(parse(&a(&["--headless"])).is_none());
    }

    #[test]
    fn a_run_is_parsed_with_its_options() {
        let r = parse(&a(&["run", "fix", "the", "bug", "--cwd", "/x", "--model", "nebius/m", "--max-minutes", "5", "--out", "o.json"])).unwrap().unwrap();
        assert_eq!(r.task, "fix the bug");
        assert_eq!(r.cwd, "/x");
        assert_eq!(r.model.as_deref(), Some("nebius/m"));
        assert_eq!(r.max_minutes, 5);
        assert_eq!(r.out.as_deref(), Some("o.json"));
    }

    #[test]
    fn bad_runs_are_refused() {
        assert!(parse(&a(&["run"])).unwrap().is_err());
        assert!(parse(&a(&["run", "x", "--max-minutes", "0"])).unwrap().is_err());
        assert!(parse(&a(&["run", "x", "--bogus"])).unwrap().is_err());
        assert!(parse(&a(&["run", "x", "--cwd"])).unwrap().is_err());
    }

    #[test]
    fn every_listed_provider_resolves() {
        for (name, _) in PROVIDERS {
            let m = model_from(&format!("{name}/some-model")).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(m.provider, *name);
        }
        assert!(help().contains("anthropic"));
        assert!(wants_help(&a(&["run", "--help"])));
        assert!(!wants_help(&a(&["--help"])));
    }

    #[test]
    fn auto_selects_the_router() {
        for name in ["auto", "router", "router/viv-auto"] {
            assert!(crate::agent::models::routing::is_router(&model_from(name).unwrap()), "{name}");
        }
    }

    #[test]
    fn a_model_needs_a_provider() {
        assert!(model_from("no-slash").is_err());
        assert!(model_from("not-a-provider/x").is_err());
    }
}

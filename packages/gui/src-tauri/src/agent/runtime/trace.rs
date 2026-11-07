//! A trace of every run, one JSON line per event, so a bad run can be read
//! back step by step instead of guessed at: which model answered each turn
//! (and whether it was a fallback), how big the context was, when history
//! was masked, compacted or had images evicted, what each tool call did,
//! and which reminders the loop injected.
//!
//! Written to `~/.viv/traces/<yyyy-mm-dd>/<run>.jsonl`, best-effort: a trace
//! that can't be written never affects the run. Kept 14 days.

use std::io::Write;
use std::path::PathBuf;

const KEEP_DAYS: u64 = 14;

pub struct RunTrace {
    file: Option<std::fs::File>,
    pub path: Option<PathBuf>,
    step: usize,
    /// What was last written in full, so an unchanged system prompt or tool list isn't repeated on every step.
    last_prompt: u64,
    last_tools: u64,
    /// How many messages the last request had, so each request lists only what is new in it.
    last_len: usize,
}

fn hash_of(s: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}

/// The first `max` characters of `s`, with a mark when it was cut.
pub fn preview(s: &str, max: usize) -> String {
    let mut out: String = s.chars().take(max).collect();
    if s.chars().count() > max {
        out.push('…');
    }
    out
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

fn day(ms: u64) -> String {
    // Days since the epoch to a civil date (Howard Hinnant's algorithm), so
    // the folder name needs no date library.
    let z = (ms / 86_400_000) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Delete trace folders older than `KEEP_DAYS`.
fn prune(root: &std::path::Path) {
    let cutoff = day(now_ms().saturating_sub(KEEP_DAYS * 86_400_000));
    if let Ok(dirs) = std::fs::read_dir(root) {
        for d in dirs.flatten() {
            let name = d.file_name().to_string_lossy().to_string();
            if name.len() == 10 && name < cutoff {
                let _ = std::fs::remove_dir_all(d.path());
            }
        }
    }
}

impl RunTrace {
    /// Open a trace for a run. `source` names the conversation it belongs to.
    pub fn start(source: &str, model: &str) -> Self {
        let root = crate::agent::system::paths::viv_home_path(&["traces"]);
        let dir = root.join(day(now_ms()));
        let safe: String = source.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '_' }).take(48).collect();
        let path = dir.join(format!("{}-{}-{}.jsonl", now_ms(), if safe.is_empty() { "main" } else { &safe }, &uuid::Uuid::new_v4().to_string()[..8]));
        let file = std::fs::create_dir_all(&dir).ok().and_then(|_| std::fs::OpenOptions::new().create(true).append(true).open(&path).ok());
        if file.is_some() {
            prune(&root);
        }
        let mut t = RunTrace { file, path: Some(path), step: 0, last_prompt: 0, last_tools: 0, last_len: 0 };
        t.event("run_start", serde_json::json!({ "source": source, "model": model }));
        t
    }

    pub fn set_step(&mut self, step: usize) {
        self.step = step;
    }

    /// What this step sent to the model: the system prompt and the tools offered (each only when they changed since the last step), and the
    /// messages added to the conversation since the last request, as short previews. Enough to read back why a model did what it did.
    pub fn request(&mut self, system_prompt: &str, tools: &[serde_json::Value], messages: &[crate::agent::runtime::types::AgentMessage]) {
        use crate::agent::runtime::types::{AgentMessage, MessageContent};
        let prompt_hash = hash_of(system_prompt);
        let tool_list: Vec<serde_json::Value> = tools
            .iter()
            .map(|t| {
                serde_json::json!({
                    "name": t.get("name").and_then(|n| n.as_str()).unwrap_or(""),
                    "description": preview(t.get("description").and_then(|n| n.as_str()).unwrap_or(""), 400),
                })
            })
            .collect();
        let tools_hash = hash_of(&serde_json::to_string(&tool_list).unwrap_or_default());
        let from = if self.last_len <= messages.len() { self.last_len } else { 0 };
        let text_of = |content: &[MessageContent]| -> String {
            content.iter().filter_map(|c| if let MessageContent::Text { text } = c { Some(text.as_str()) } else { None }).collect::<Vec<_>>().join("\n")
        };
        let fresh: Vec<serde_json::Value> = messages[from..]
            .iter()
            .map(|m| match m {
                AgentMessage::User { content, .. } => serde_json::json!({ "role": "user", "text": preview(&text_of(content), 1500) }),
                AgentMessage::Assistant { content, .. } => serde_json::json!({
                    "role": "assistant",
                    "text": preview(&text_of(content), 1500),
                    "calls": content.iter().filter_map(|c| if let MessageContent::ToolCall(tc) = c { Some(tc.name.clone()) } else { None }).collect::<Vec<_>>(),
                }),
                AgentMessage::ToolResult { tool_name, content, is_error, .. } => {
                    serde_json::json!({ "role": "tool", "tool": tool_name, "failed": *is_error == Some(true), "text": preview(&text_of(content), 600) })
                }
                _ => serde_json::json!({ "role": "other" }),
            })
            .collect();
        let mut fields = serde_json::json!({ "messages_total": messages.len(), "new_messages": fresh });
        if prompt_hash != self.last_prompt {
            fields["system_prompt"] = serde_json::json!(system_prompt);
        }
        if tools_hash != self.last_tools {
            fields["tools"] = serde_json::json!(tool_list);
        }
        self.last_prompt = prompt_hash;
        self.last_tools = tools_hash;
        self.last_len = messages.len();
        self.event("request", fields);
    }

    /// Append one event: `{ "t": ms, "step": n, "kind": ..., ...fields }`.
    pub fn event(&mut self, kind: &str, fields: serde_json::Value) {
        let Some(f) = self.file.as_mut() else { return };
        let mut line = serde_json::json!({ "t": now_ms(), "step": self.step, "kind": kind });
        if let (Some(obj), serde_json::Value::Object(extra)) = (line.as_object_mut(), fields) {
            obj.extend(extra);
        }
        let _ = writeln!(f, "{line}");
    }
}

/// One run's trace, as the Trajectory view lists it.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TraceInfo {
    /// The file's path, handed back to `cmd_trace_read`.
    pub path: String,
    pub started_ms: u64,
    pub source: String,
    pub bytes: u64,
}

/// Recent runs, newest first.
fn list_in(root: &std::path::Path, limit: usize) -> Vec<TraceInfo> {
    let mut out = Vec::new();
    for d in std::fs::read_dir(root).into_iter().flatten().flatten() {
        for f in std::fs::read_dir(d.path()).into_iter().flatten().flatten() {
            let name = f.file_name().to_string_lossy().to_string();
            let Some(stem) = name.strip_suffix(".jsonl") else { continue };
            let mut parts = stem.splitn(2, '-');
            let started_ms = parts.next().and_then(|t| t.parse().ok()).unwrap_or(0);
            // `<ms>-<source>-<id>`: the source is everything between the time and the trailing id.
            let rest = parts.next().unwrap_or("");
            let source = rest.rsplit_once('-').map(|(s, _)| s).unwrap_or(rest).to_string();
            out.push(TraceInfo { path: f.path().to_string_lossy().to_string(), started_ms, source, bytes: f.metadata().map(|m| m.len()).unwrap_or(0) });
        }
    }
    out.sort_by(|a, b| b.started_ms.cmp(&a.started_ms));
    out.truncate(limit);
    out
}

/// Read a trace's events. Only files under `root` can be read, and a huge one is cut at `max_bytes`.
fn read_in(root: &std::path::Path, path: &str, max_bytes: u64) -> Result<Vec<serde_json::Value>, String> {
    let root = root.canonicalize().map_err(|e| e.to_string())?;
    let file = std::path::Path::new(path).canonicalize().map_err(|_| "That trace isn't there any more.".to_string())?;
    if !file.starts_with(&root) {
        return Err("That isn't one of Vivianne's traces.".into());
    }
    use std::io::Read;
    let mut text = String::new();
    std::fs::File::open(&file).map_err(|e| e.to_string())?.take(max_bytes).read_to_string(&mut text).map_err(|e| e.to_string())?;
    Ok(text.lines().filter_map(|l| serde_json::from_str(l).ok()).collect())
}

#[tauri::command]
pub async fn cmd_traces_list() -> Result<Vec<TraceInfo>, String> {
    Ok(list_in(&crate::agent::system::paths::viv_home_path(&["traces"]), 200))
}

#[tauri::command]
pub async fn cmd_trace_read(path: String) -> Result<Vec<serde_json::Value>, String> {
    read_in(&crate::agent::system::paths::viv_home_path(&["traces"]), &path, 12_000_000)
}

#[cfg(test)]
mod read_tests {
    use super::*;

    #[test]
    fn traces_are_listed_newest_first_and_only_files_under_the_root_can_be_read() {
        let root = std::env::temp_dir().join(format!("viv-traces-{}", std::process::id()));
        let day = root.join("2026-11-07");
        std::fs::create_dir_all(&day).unwrap();
        std::fs::write(day.join("1000-main-abcd1234.jsonl"), "{\"kind\":\"run_start\"}\n{\"kind\":\"turn\"}\n").unwrap();
        std::fs::write(day.join("2000-agent-x-ef567890.jsonl"), "{\"kind\":\"run_start\"}\n").unwrap();
        let list = list_in(&root, 10);
        assert_eq!(list.iter().map(|t| t.started_ms).collect::<Vec<_>>(), vec![2000, 1000]);
        assert_eq!(list[0].source, "agent-x");
        assert_eq!(read_in(&root, &list[1].path, 1_000_000).unwrap().len(), 2);
        let outside = std::env::temp_dir().join("viv-outside.jsonl");
        std::fs::write(&outside, "{}\n").unwrap();
        assert!(read_in(&root, &outside.to_string_lossy(), 1_000_000).is_err());
    }
}

#[cfg(test)]
mod request_tests {
    use super::*;
    use crate::agent::runtime::types::{AgentMessage, MessageContent};

    fn user(t: &str) -> AgentMessage {
        AgentMessage::User { content: vec![MessageContent::Text { text: t.into() }], timestamp: 0 }
    }

    #[test]
    fn an_unchanged_prompt_and_tool_list_are_written_once_and_each_request_lists_only_new_messages() {
        let dir = std::env::temp_dir().join(format!("viv-trace-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.jsonl");
        let file = std::fs::OpenOptions::new().create(true).append(true).open(&path).ok();
        let mut t = RunTrace { file, path: Some(path.clone()), step: 0, last_prompt: 0, last_tools: 0, last_len: 0 };
        let tools = vec![serde_json::json!({ "name": "read", "description": "Read a file" })];
        t.request("be helpful", &tools, &[user("one")]);
        t.request("be helpful", &tools, &[user("one"), user("two")]);
        let lines: Vec<serde_json::Value> = std::fs::read_to_string(&path).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect();
        assert!(lines[0].get("system_prompt").is_some() && lines[0].get("tools").is_some());
        assert!(lines[1].get("system_prompt").is_none() && lines[1].get("tools").is_none(), "unchanged, so not repeated");
        assert_eq!(lines[1]["new_messages"].as_array().unwrap().len(), 1);
        assert_eq!(lines[1]["new_messages"][0]["text"], "two");
    }

    #[test]
    fn a_preview_marks_where_it_was_cut() {
        assert_eq!(preview("hello", 10), "hello");
        assert_eq!(preview("hello world", 5), "hello…");
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn civil_dates() {
        assert_eq!(super::day(0), "1970-01-01");
        assert_eq!(super::day(1_793_059_200_000), "2026-10-27");
    }
}

//! Memory tools — surface Vivianne's existing SQLite/FTS5 memory store to the
//! model, mirroring grok's `memory_get` / `memory_search`.
//!
//! Vivianne already has a well-built memory subsystem
//! (`crate::agent::memory` + `memory_store`) with BM25 FTS5 lookup and RRF
//! hybrid retrieval; these tools are thin adapters that let the coding agent
//! call it directly instead of relying on the auto-inject-on-turn path.

use async_trait::async_trait;
use serde_json::json;

use super::tool_exec::{Tool, ToolResult};

pub struct MemoryGetTool {
    cwd: String,
}

impl MemoryGetTool {
    pub fn new(cwd: String) -> Self { Self { cwd } }
}

#[async_trait]
impl Tool for MemoryGetTool {
    fn name(&self) -> &str { "memory_get" }

    fn description(&self) -> &str {
        "Return the current memory-context bundle for this workspace: distilled facts and prior-session summaries that the memory subsystem would inject at turn start. Use to check what long-lived context you already have before assuming you need to re-derive it. Returns empty when memory is disabled or has no entries."
    }

    fn parameters(&self) -> serde_json::Value {
        json!({ "type": "object", "properties": {} })
    }

    async fn execute(&self, _args: &serde_json::Value, _cwd: &str) -> ToolResult {
        match crate::agent::memory::context(&self.cwd).await {
            Some((ctx, entry_count)) if !ctx.trim().is_empty() => ToolResult {
                content: ctx,
                is_error: false,
                details: Some(json!({ "source": "memory.context", "entry_count": entry_count })),
            },
            _ => ToolResult {
                content: "Memory has no entries (or is disabled for this workspace).".to_string(),
                is_error: false,
                details: Some(json!({ "empty": true })),
            },
        }
    }
}

pub struct MemorySearchTool {
    cwd: String,
}

impl MemorySearchTool {
    pub fn new(cwd: String) -> Self { Self { cwd } }
}

#[async_trait]
impl Tool for MemorySearchTool {
    fn name(&self) -> &str { "memory_search" }

    fn description(&self) -> &str {
        "Search stored memories by keyword using BM25 (FTS5). Returns matching memory entries — facts, prior-session summaries, user preferences — ranked by relevance. Cheaper than a fresh code search when the answer is likely already known from earlier turns."
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "Keywords or a short natural-language query" },
                "limit": { "type": "number", "description": "Max hits to return (default 8)" }
            },
            "required": ["query"]
        })
    }

    async fn execute(&self, args: &serde_json::Value, _cwd: &str) -> ToolResult {
        let query = args["query"].as_str().unwrap_or("").trim().to_string();
        if query.is_empty() {
            return ToolResult {
                content: "'query' must not be empty".to_string(),
                is_error: true,
                details: None,
            };
        }
        let limit = args["limit"].as_u64().unwrap_or(8).max(1).min(50) as usize;

        // FTS BM25 only — the vector path in `recall()` needs an embedding call
        // and an `AuthState`, which we don't have here. FTS covers the same
        // "known text I've stored" query with no dependency on the model.
        let hits = crate::agent::memory_store::search_fts(&self.cwd, &query, limit)
            .await
            .unwrap_or_default();
        if hits.is_empty() {
            return ToolResult {
                content: format!("No memories found for: {query}"),
                is_error: false,
                details: Some(json!({ "query": query, "hits": 0 })),
            };
        }

        let mut out = format!("Found {} memory hit(s) for: {query}\n", hits.len());
        let mut json_hits = Vec::new();
        for e in &hits {
            out.push_str(&format!(
                "\n- [{kind}] {key}\n  {text}\n",
                kind = e.kind,
                key = e.key,
                text = truncate_display(&e.text, 400),
            ));
            json_hits.push(json!({
                "id": e.id,
                "key": e.key,
                "kind": e.kind,
                "text": e.text,
                "session_id": e.session_id,
                "created_at": e.created_at,
            }));
        }
        ToolResult {
            content: out,
            is_error: false,
            details: Some(json!({ "query": query, "hits": json_hits })),
        }
    }
}

pub struct NotesWriteTool {
    cwd: String,
}

impl NotesWriteTool {
    pub fn new(cwd: String) -> Self { Self { cwd } }
}

#[async_trait]
impl Tool for NotesWriteTool {
    fn name(&self) -> &str { "notes_write" }

    fn description(&self) -> &str {
        "Append a freeform note to this workspace's `.vivianne/notes.md` scratchpad — for anything worth writing down that isn't a structured memory fact (a running observation, a thing to check later, context for your future self). Unlike `remember`, this is plain prose with no confidence/decay/retrieval scoring; it's just a file you can also read/edit directly."
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "text": { "type": "string", "description": "The note to append" }
            },
            "required": ["text"]
        })
    }

    async fn execute(&self, args: &serde_json::Value, _cwd: &str) -> ToolResult {
        let text = args["text"].as_str().unwrap_or("").trim().to_string();
        if text.is_empty() {
            return ToolResult {
                content: "'text' must not be empty".to_string(),
                is_error: true,
                details: None,
            };
        }
        let text = crate::agent::memory::redact_secrets(&text);
        let path = std::path::Path::new(&self.cwd).join(".vivianne").join("notes.md");
        let dir = path.parent().map(|p| p.to_path_buf());
        let stamp = chrono::Local::now().format("%Y-%m-%d %H:%M").to_string();
        let entry = format!("\n## {stamp}\n\n{text}\n");
        let result = tokio::task::spawn_blocking(move || -> std::io::Result<()> {
            if let Some(dir) = dir {
                std::fs::create_dir_all(&dir)?;
            }
            use std::io::Write;
            let mut file = std::fs::OpenOptions::new().create(true).append(true).open(&path)?;
            file.write_all(entry.as_bytes())
        })
        .await;
        match result {
            Ok(Ok(())) => ToolResult {
                content: "Note appended to .vivianne/notes.md".to_string(),
                is_error: false,
                details: Some(json!({ "path": ".vivianne/notes.md" })),
            },
            _ => ToolResult {
                content: "Failed to write note".to_string(),
                is_error: true,
                details: None,
            },
        }
    }
}

fn truncate_display(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    let mut out: String = text.chars().take(limit).collect();
    out.push_str(" …[truncated]");
    out
}

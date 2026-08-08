//! Static tool schema definitions (JSON parameter specs) sent to the model.
//! Pure data, no dispatch logic — split out of tool_exec.rs (which mixes
//! schemas, the execution loop, and per-tool handlers) so the two concerns
//! don't have to be scrolled through together.

pub(super) fn static_tool_definitions() -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({
            "name": "read",
            "description": "Read the contents of a file. Handles plain text, PDFs (page count + extracted text via 'pages' param), images (format + pixel dimensions + optional OCR), and Jupyter notebooks (.ipynb, cell-by-cell). For text files, pass offset/limit for partial reads. For images, set ocr=true to extract text via Baidu Unlimited-OCR (requires HUGGINGFACE_TOKEN env var).",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Relative path of the file" },
                    "offset": { "type": "number", "description": "Start reading at this line (1-indexed) — text files only" },
                    "limit": { "type": "number", "description": "Maximum lines to read — text files only" },
                    "pages": { "type": "string", "description": "PDF page range, e.g. \"1-5\" or \"3\" — PDF files only" },
                    "ocr": { "type": "boolean", "description": "If true, run OCR on image files to extract text (requires HUGGINGFACE_TOKEN). Uses Baidu Unlimited-OCR via HF Inference API, with local PaddleOCR fallback." }
                },
                "required": ["path"]
            }
        }),
        serde_json::json!({
            "name": "write",
            "description": "Write content to a file, creating it if it doesn't exist.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Relative path of the file" },
                    "content": { "type": "string", "description": "Content to write" }
                },
                "required": ["path", "content"]
            }
        }),
        serde_json::json!({
            "name": "edit",
            "description": "Replace a specific substring in a file with new text.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Relative path of the file" },
                    "old_string": { "type": "string", "description": "Sub-string to be replaced" },
                    "new_string": { "type": "string", "description": "New content to insert" }
                },
                "required": ["path", "old_string", "new_string"]
            }
        }),
        serde_json::json!({
            "name": "bash",
            "description": "Execute a bash command in the workspace.",
            "parameters": {
                "type": "object",
                "properties": {
                    "command": { "type": "string", "description": "Shell command to execute" }
                },
                "required": ["command"]
            }
        }),
        serde_json::json!({
            "name": "ls",
            "description": "List the contents of a directory. By default shows a flat listing; add 'depth' for a hierarchical, gitignore-aware tree view.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Relative path to list" },
                    "depth": { "type": "number", "description": "If set, show a hierarchical tree up to this depth (default 3, max 6) instead of a flat list" },
                    "show_hidden": { "type": "boolean", "description": "Include dotfiles in output (default false)" }
                },
                "required": []
            }
        }),
        serde_json::json!({
            "name": "grep",
            "description": "Search for a regex pattern in files recursively.",
            "parameters": {
                "type": "object",
                "properties": {
                    "pattern": { "type": "string", "description": "Regex pattern to search for" },
                    "path": { "type": "string", "description": "Directory or file to search in" }
                },
                "required": ["pattern"]
            }
        }),
        serde_json::json!({
            "name": "code_search",
            "description": "Search the codebase by meaning/relevance rather than exact text — good for loose or conceptual queries (\"where do we handle authentication\", \"rate limiting logic\") where you don't know the exact string to grep for. Ranks files by relevance. For an exact known string or regex, use grep instead.",
            "parameters": {
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Natural-language or keyword description of what you're looking for" },
                    "path": { "type": "string", "description": "Directory to search in (default: whole workspace)" }
                },
                "required": ["query"]
            }
        }),
        serde_json::json!({
            "name": "computer",
            "description": "Control the computer: move and click the mouse, type text, press keys, and take screenshots. Take a screenshot first to orient yourself before clicking.",
            "parameters": {
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": [
                            "key", "type", "mouse_move", "left_click", "left_click_drag",
                            "right_click", "middle_click", "double_click", "screenshot", "cursor_position"
                        ],
                        "description": "The action to perform."
                    },
                    "coordinate": {
                        "type": "array",
                        "items": { "type": "number" },
                        "description": "X,Y screen coordinates for mouse actions."
                    },
                    "text": { "type": "string", "description": "Text to type, or key combination such as 'Control+C'." }
                },
                "required": ["action"]
            }
        }),
        serde_json::json!({
            "name": "glob",
            "description": "Find files by name pattern or wildcard (e.g. '**/*.ts', 'src/*.rs').",
            "parameters": {
                "type": "object",
                "properties": {
                    "pattern": { "type": "string", "description": "Glob pattern (supports * and ?)" },
                    "path": { "type": "string", "description": "Directory to search in" }
                },
                "required": ["pattern"]
            }
        }),
        serde_json::json!({
            "name": "repo_clone",
            "description": "Clone (or reuse a cached clone of) a remote git repository so its files can be read/searched. Accepts a git URL, 'owner/repo' GitHub shorthand, or 'host/owner/repo'.",
            "parameters": {
                "type": "object",
                "properties": {
                    "repository": { "type": "string", "description": "Git URL, 'owner/repo', or 'host/owner/repo'" },
                    "refresh": { "type": "boolean", "description": "Fetch the latest remote state into the cache" },
                    "branch": { "type": "string", "description": "Branch or ref to check out" }
                },
                "required": ["repository"]
            }
        }),
        serde_json::json!({
            "name": "repo_overview",
            "description": "Get a structured overview of a directory or cached repository: file tree, detected language/ecosystem, package manager, dependency files, and likely entrypoints.",
            "parameters": {
                "type": "object",
                "properties": {
                    "repository": { "type": "string", "description": "A repository already fetched with repo_clone" },
                    "path": { "type": "string", "description": "Directory path to inspect instead of a cached repository" },
                    "depth": { "type": "number", "description": "Max structure depth to include (1-6, default 3)" }
                }
            }
        }),
        serde_json::json!({
            "name": "powershell",
            "description": "Execute a PowerShell command. Preferred on Windows for shell operations.",
            "parameters": {
                "type": "object",
                "properties": {
                    "command": { "type": "string", "description": "PowerShell command to execute" },
                    "timeout": { "type": "number", "description": "Timeout in seconds (default 120, max 600)" }
                },
                "required": ["command"]
            }
        }),
        serde_json::json!({
            "name": "notebook_edit",
            "description": "Edit a Jupyter notebook (.ipynb): replace, insert, or delete a cell's source.",
            "parameters": {
                "type": "object",
                "properties": {
                    "notebook_path": { "type": "string", "description": "Path to the .ipynb file" },
                    "cell_id": { "type": "string", "description": "ID or numeric index of the cell" },
                    "new_source": { "type": "string", "description": "New source for the cell" },
                    "cell_type": { "type": "string", "enum": ["code", "markdown"], "description": "Cell type (required for insert)" },
                    "edit_mode": { "type": "string", "enum": ["replace", "insert", "delete"], "description": "Edit mode (default replace)" }
                },
                "required": ["notebook_path", "new_source"]
            }
        }),
        serde_json::json!({
            "name": "diagnostics",
            "description": "Get language-server diagnostics (errors/warnings) for a file. Requires a language server for that file type to be installed and on PATH.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Path to the file to check" }
                },
                "required": ["path"]
            }
        }),
        serde_json::json!({
            "name": "remember",
            "description": "Save a fact or note to persistent project memory, so it's available in future sessions on this project.",
            "parameters": {
                "type": "object",
                "properties": {
                    "text": { "type": "string", "description": "The fact or note to remember" },
                    "key": { "type": "string", "description": "Optional short key to file it under" }
                },
                "required": ["text"]
            }
        }),
        serde_json::json!({
            "name": "recall",
            "description": "Search persistent project memory for facts or notes saved in previous sessions.",
            "parameters": {
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "What to search memory for" }
                },
                "required": ["query"]
            }
        }),
        serde_json::json!({
            "name": "read_skill",
            "description": "Read the full instructions for a skill listed in your skills catalog, by name. Call this before attempting a task a registered skill covers.",
            "parameters": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "The skill's name, as listed in the skills catalog" }
                },
                "required": ["name"]
            }
        }),
        serde_json::json!({
            "name": "web_fetch",
            "description": "Fetch a URL and return its text content (HTML stripped to readable text).",
            "parameters": {
                "type": "object",
                "properties": {
                    "url": { "type": "string", "description": "The URL to fetch" },
                    "prompt": { "type": "string", "description": "Optional focus for what to extract" }
                },
                "required": ["url"]
            }
        }),
        serde_json::json!({
            "name": "web_preview",
            "description": "Open a URL, or a local file the browser can render (HTML, PDF, SVG, images), in a live preview panel inside the app — a real screenshot from an actual browser (camoufox-browser), not a sandboxed iframe, so it works even for sites/pages that block being framed. Use web_fetch instead when you just need to read the content as text yourself. Requires camoufox-browser running (npx @askjo/camofox-browser) — if it isn't, this returns a clear error explaining how to start it.",
            "parameters": {
                "type": "object",
                "properties": {
                    "url": { "type": "string", "description": "http(s):// URL, file:// URL, or a bare local path like \"C:\\report.html\" or \"/tmp/output.pdf\" (auto-converted to file://)" }
                },
                "required": ["url"]
            }
        }),
        serde_json::json!({
            "name": "web_search",
            "description": "Search the web and return result titles and URLs. Follow up with web_fetch to read a result.",
            "parameters": {
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "The search query" }
                },
                "required": ["query"]
            }
        }),
        serde_json::json!({
            "name": "sleep",
            "description": "Pause for a number of seconds (max 60), e.g. to wait for a server to start.",
            "parameters": {
                "type": "object",
                "properties": {
                    "seconds": { "type": "number", "description": "Seconds to sleep (max 60)" }
                },
                "required": ["seconds"]
            }
        }),
        serde_json::json!({
            "name": "apply_patch",
            "description": "Apply a multi-file patch in the 'V4A' format (*** Begin Patch / Add File / Update File / Delete File / End Patch). Add, edit, move, or delete several files (or several regions of one file) in a single call instead of many edit calls.",
            "parameters": {
                "type": "object",
                "properties": {
                    "patchText": { "type": "string", "description": "The full patch text (must start with '*** Begin Patch' and end with '*** End Patch')" }
                },
                "required": ["patchText"]
            }
        }),
        serde_json::json!({
            "name": "question",
            "description": "Ask the user 1-4 multiple-choice questions and wait for their answers before continuing. Use for genuine decision points, not to avoid making a reasonable judgment call.",
            "parameters": {
                "type": "object",
                "properties": {
                    "questions": {
                        "type": "array",
                        "minItems": 1,
                        "maxItems": 4,
                        "items": {
                            "type": "object",
                            "properties": {
                                "question": { "type": "string" },
                                "header": { "type": "string", "description": "Short chip label, e.g. 'Auth method'" },
                                "options": {
                                    "type": "array",
                                    "minItems": 2,
                                    "maxItems": 4,
                                    "items": {
                                        "type": "object",
                                        "properties": {
                                            "label": { "type": "string" },
                                            "description": { "type": "string" }
                                        },
                                        "required": ["label", "description"]
                                    }
                                },
                                "multiSelect": { "type": "boolean" }
                            },
                            "required": ["question", "header", "options"]
                        }
                    }
                },
                "required": ["questions"]
            }
        }),
        serde_json::json!({
            "name": "confirm_action",
            "description": "Ask the user a single yes/no confirmation before proceeding, and wait for their answer. Use this for actions you're about to take that are irreversible or consequential enough to deserve an explicit check-in, even if the specific tool call itself isn't one of the ones that already prompts for approval — e.g. before deleting real data, sending something externally-visible, or making a call the user hasn't clearly authorized yet. Not for routine decisions — use your judgment for those.",
            "parameters": {
                "type": "object",
                "properties": {
                    "message": { "type": "string", "description": "The specific action to confirm, phrased as a yes/no question, e.g. \"Delete all 40 rows matching this filter?\"" },
                    "danger_level": { "type": "string", "enum": ["low", "medium", "high"], "description": "How consequential/hard-to-reverse this is — the user sees this reflected in the dialog's styling. Default \"medium\"." }
                },
                "required": ["message"]
            }
        }),
        serde_json::json!({
            "name": "present_plan",
            "description": "Show the user a structured, numbered plan of the approach you're about to take, before you start executing it — for up-front buy-in on a non-trivial task. This is a one-time presentation, not an ongoing tracker (use todo_write for tracking execution progress once you've started). Call this once, near the start of a task that has multiple meaningful steps or a real design decision behind it — skip it for small, obvious asks.",
            "parameters": {
                "type": "object",
                "properties": {
                    "title": { "type": "string", "description": "Short label for the plan, e.g. \"Add dark mode support\"" },
                    "steps": { "type": "array", "items": { "type": "string" }, "minItems": 1, "description": "Ordered plan steps, each a short phrase" }
                },
                "required": ["title", "steps"]
            }
        }),
        serde_json::json!({
            "name": "present_snippet",
            "description": "Show the user a short, syntax-highlighted code snippet inline in the chat, without writing it to a file. Use for a quick example, a one-off calculation, or illustrating an idea — not for real changes to the user's project (use write/edit for those).",
            "parameters": {
                "type": "object",
                "properties": {
                    "code": { "type": "string", "description": "The snippet's source code" },
                    "language": { "type": "string", "description": "Language for syntax highlighting, e.g. \"typescript\", \"python\", \"rust\" (default \"text\")" },
                    "description": { "type": "string", "description": "One-line caption for what the snippet shows" }
                },
                "required": ["code"]
            }
        }),
        serde_json::json!({
            "name": "todo_write",
            "description": "Create and manage a structured task checklist for the session. Replaces the whole list each call.",
            "parameters": {
                "type": "object",
                "properties": {
                    "todos": {
                        "type": "array",
                        "description": "The full updated todo list",
                        "items": {
                            "type": "object",
                            "properties": {
                                "content": { "type": "string" },
                                "status": { "type": "string", "enum": ["pending", "in_progress", "completed"] }
                            },
                            "required": ["content", "status"]
                        }
                    }
                },
                "required": ["todos"]
            }
        }),
        serde_json::json!({
            "name": "generate_image",
            "description": "Generate a new image from a text description using an image-generation model. Defaults to OpenAI's gpt-image-1; pass provider=\"google\" to use Google's Imagen instead. Requires an API key for whichever provider is used, configured in Settings → API Keys — if none is set, this returns an error saying so. Use for creating original images (illustrations, mockups, icons), not for analysing an existing image (that's just done by looking at it, no tool needed).",
            "parameters": {
                "type": "object",
                "properties": {
                    "prompt": { "type": "string", "description": "Description of the image to generate" },
                    "size": { "type": "string", "description": "OpenAI only. Image size, e.g. \"1024x1024\", \"1536x1024\", \"1024x1536\" (default \"1024x1024\")" },
                    "provider": { "type": "string", "enum": ["openai", "google"], "description": "Which provider to generate with (default \"openai\")" },
                    "model": { "type": "string", "description": "Override the default model for the chosen provider, e.g. \"dall-e-3\" for openai or \"imagen-4.0-generate-001\" for google" }
                },
                "required": ["prompt"]
            }
        }),
        serde_json::json!({
            "name": "connect_mcp_server",
            "description": "Connect to a Model Context Protocol (MCP) server, making its tools available to you (as mcp__<name>__<tool>) on your very next turn, and persisting the connection so it survives an app restart. It also then shows up in Settings → MCP Servers. Use this when the user asks you to connect to, set up, or try an MCP server — you do not need to ask them to do it manually via Settings.",
            "parameters": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Unique short name to identify this server, e.g. \"github\" or \"weather\"" },
                    "transport": { "type": "string", "enum": ["stdio", "http"], "description": "\"stdio\" for a local command-line MCP server (most npm/pip-installed servers), \"http\" for a remote MCP server reachable over HTTP" },
                    "command": { "type": "string", "description": "transport=stdio only: the command to run, e.g. \"npx\"" },
                    "args": { "type": "array", "items": { "type": "string" }, "description": "transport=stdio only: command-line arguments, e.g. [\"-y\", \"@some/mcp-server\"]" },
                    "env": { "type": "object", "description": "transport=stdio only: environment variables to pass to the server process, e.g. an API key it needs" },
                    "url": { "type": "string", "description": "transport=http only: the server's URL" },
                    "headers": { "type": "object", "description": "transport=http only: HTTP headers to send, e.g. an Authorization header" }
                },
                "required": ["name", "transport"]
            }
        }),
        serde_json::json!({
            "name": "get_current_time",
            "description": "Get the current date/time (ISO 8601, with the machine's local timezone offset) and day of week. Call this before computing a cron expression or reasoning about a relative time ('tomorrow', 'in 2 hours', 'next Monday') — you do not otherwise know what time it is right now.",
            "parameters": { "type": "object", "properties": {} }
        }),
        serde_json::json!({
            "name": "create_automation",
            "description": "Schedule a prompt to be run through the agent automatically on a cron schedule. Creates it enabled. It appears in Settings → Automations, where the user can also see/edit/disable it. Call get_current_time first if the schedule is relative ('every morning at 9', 'in an hour') so the cron expression lines up with the actual current time — and use the `question` tool to ask the user for the schedule/time if they haven't specified one.",
            "parameters": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Short display name for the automation" },
                    "prompt": { "type": "string", "description": "The prompt sent to the agent each time this fires" },
                    "cron": { "type": "string", "description": "Standard 5-field cron: \"min hour day-of-month month day-of-week\", e.g. \"0 9 * * *\" for 9am daily, \"*/15 * * * *\" for every 15 minutes, \"0 9 * * 1\" for 9am every Monday" }
                },
                "required": ["name", "prompt", "cron"]
            }
        }),
        serde_json::json!({
            "name": "list_automations",
            "description": "List every scheduled automation (name, prompt, cron, enabled state, last/next run time).",
            "parameters": { "type": "object", "properties": {} }
        }),
        serde_json::json!({
            "name": "delete_automation",
            "description": "Permanently delete a scheduled automation by id (see list_automations for ids).",
            "parameters": {
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "The automation's id" }
                },
                "required": ["id"]
            }
        }),
        serde_json::json!({
            "name": "set_automation_enabled",
            "description": "Enable or disable a scheduled automation without deleting it.",
            "parameters": {
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "The automation's id" },
                    "enabled": { "type": "boolean" }
                },
                "required": ["id", "enabled"]
            }
        }),
        serde_json::json!({
            "name": "task",
            "description": "Launch a subagent with a self-contained prompt. By default (run_in_background=true) this returns a task_id immediately — poll or block on it with task_output/wait_tasks — instead of blocking your own turn, and it shows up live in the Background Tasks panel so the user can see it running. Call this once per subagent — spawn multiple concurrently rather than looping serial blocking calls. Set run_in_background=false only for a single one-off subtask where you have nothing else to do until it returns. Single-level depth cap — a subagent spawned by task cannot itself call task again.",
            "parameters": {
                "type": "object",
                "properties": {
                    "description": { "type": "string", "description": "Short (3-5 word) description of the task" },
                    "prompt": { "type": "string", "description": "Self-contained instructions for the subagent — it does not see this conversation" },
                    "subagent_type": { "type": "string", "description": "Label for the subagent's role (default 'general-purpose')", "default": "general-purpose" },
                    "run_in_background": { "type": "boolean", "description": "If true (default), return a task_id immediately instead of blocking", "default": true }
                },
                "required": ["description", "prompt"]
            }
        }),
        serde_json::json!({
            "name": "task_output",
            "description": "Poll (or wait for) one or more background subagents launched with task. Returns a status snapshot per id: running, completed (with output), failed, or cancelled. Optional timeout_ms blocks until at least one id is terminal.",
            "parameters": {
                "type": "object",
                "properties": {
                    "task_ids": { "type": "array", "items": { "type": "string" }, "description": "Subagent ids returned by task" },
                    "timeout_ms": { "type": "number", "description": "If set, block up to this many milliseconds for one id to finish (default 0 = no wait)" }
                },
                "required": ["task_ids"]
            }
        }),
        serde_json::json!({
            "name": "wait_tasks",
            "description": "Block until subagents launched with task complete. mode='any' returns as soon as one finishes; mode='all' waits for every id. Prefer this over polling task_output in a loop.",
            "parameters": {
                "type": "object",
                "properties": {
                    "task_ids": { "type": "array", "items": { "type": "string" } },
                    "mode": { "type": "string", "enum": ["any", "all"], "default": "all" },
                    "timeout_ms": { "type": "number", "description": "Give up after this many ms and return the current snapshot" }
                },
                "required": ["task_ids"]
            }
        }),
        serde_json::json!({
            "name": "kill_task",
            "description": "Cancel one or more running subagents launched with task. Already-finished subagents are left alone. Returns a per-id outcome: cancelled, already_finished, or not_found.",
            "parameters": {
                "type": "object",
                "properties": {
                    "task_ids": { "type": "array", "items": { "type": "string" } },
                    "reason": { "type": "string", "description": "Optional human-readable reason recorded on the cancellation" }
                },
                "required": ["task_ids"]
            }
        }),
        serde_json::json!({
            "name": "enter_plan_mode",
            "description": "Enter plan mode. While active, mutating tools (write, edit, apply_patch, bash, powershell, notebook_edit, and their streaming variants) are refused — you can only read, search, and think. Use to sketch a plan before making changes. Provide an optional 'plan' summary.",
            "parameters": {
                "type": "object",
                "properties": {
                    "plan": { "type": "string", "description": "Optional summary of the plan you're proposing" }
                }
            }
        }),
        serde_json::json!({
            "name": "exit_plan_mode",
            "description": "Exit plan mode, re-enabling mutating tools. Returns the plan you recorded with enter_plan_mode, if any.",
            "parameters": { "type": "object", "properties": {} }
        }),
        serde_json::json!({
            "name": "streaming_bash",
            "description": "Same as bash but streams stdout/stderr to the GUI live as the command runs instead of buffering until completion. Use for long-running builds/tests where live progress matters; otherwise prefer plain bash.",
            "parameters": {
                "type": "object",
                "properties": {
                    "command": { "type": "string", "description": "Bash command to execute" },
                    "timeout": { "type": "number", "description": "Timeout in seconds (default 120, max 600)" }
                },
                "required": ["command"]
            }
        }),
        serde_json::json!({
            "name": "streaming_powershell",
            "description": "Same as powershell but streams stdout/stderr to the GUI live as the command runs. Use for long-running Windows commands where live progress matters; otherwise prefer plain powershell.",
            "parameters": {
                "type": "object",
                "properties": {
                    "command": { "type": "string" },
                    "timeout": { "type": "number", "description": "Timeout in seconds (default 120, max 600)" }
                },
                "required": ["command"]
            }
        }),
        serde_json::json!({
            "name": "sandbox_execute",
            "description": "Run code in an isolated, network-disabled Docker container and return its stdout/stderr — nothing it does can touch the user's actual filesystem or environment. Use this instead of bash/powershell for untrusted, exploratory, or throwaway code (testing a snippet, running something from the internet, quick calculations) where you want real isolation rather than running directly on the user's machine. Requires Docker Desktop installed and running; if it isn't, this returns a clear error rather than falling back to unsandboxed execution.",
            "parameters": {
                "type": "object",
                "properties": {
                    "language": { "type": "string", "enum": ["python", "node", "bash"], "description": "Runtime to execute the code with" },
                    "code": { "type": "string", "description": "The code to run" },
                    "timeout_seconds": { "type": "number", "description": "Max seconds before the container is killed (default 30, max 120)" }
                },
                "required": ["language", "code"]
            }
        })
    ]
}

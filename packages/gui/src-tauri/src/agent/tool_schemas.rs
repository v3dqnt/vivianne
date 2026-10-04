//! Static tool schema definitions (JSON parameter specs) sent to the model.
//! Pure data, no dispatch logic — split out of tool_exec.rs (which mixes
//! schemas, the execution loop, and per-tool handlers) so the two concerns
//! don't have to be scrolled through together.

/// Every top-level tool name a playbook must NOT be allowed to take:
/// every built-in tool's own name, plus `"computer"` — not itself a schema
/// entry, but a name `agent::tool_exec` still dispatches as a silent alias
/// for `dip` (see its match arm), so it needs to be off-limits too.
///
/// `coding_agent::playbooks::load_playbooks` is the ONE place both the
/// schema list (`agent::tool_exec::get_agent_tool_definitions`) and the
/// `Tool` registry (`coding_agent::tool_exec::create_coding_tools`) source
/// playbooks from, so filtering there — rather than in each caller — closes
/// both problems a same-named playbook would otherwise cause with one check:
/// a literal duplicate `"name"` entry in the JSON array sent to the model,
/// and a playbook that can never actually run because the registry lookup
/// returns the built-in with the same name first and stops looking.
pub(crate) fn reserved_tool_names() -> std::collections::HashSet<String> {
    let mut names: std::collections::HashSet<String> =
        static_tool_definitions().iter().filter_map(|d| d.get("name").and_then(|v| v.as_str())).map(String::from).collect();
    names.insert("computer".to_string());
    names
}

/// The `powershell` tool's description, which has to tell the truth about the
/// host it is running on.
///
/// Windows always has `powershell.exe`. Everywhere else the executor shells
/// out to `pwsh` (PowerShell Core), which is an optional install almost nobody
/// has on a Mac — so advertising it as a peer of `bash` there just invites the
/// model to spend a turn discovering it does not exist. When it IS installed,
/// it works fine and is described normally; the probe is cached for the
/// process, since a shell does not appear or vanish mid-session.
fn powershell_description() -> &'static str {
    #[cfg(target_os = "windows")]
    {
        "Execute a PowerShell command. Preferred on Windows for shell operations. Streams live output to the GUI as it runs, same as bash."
    }
    #[cfg(not(target_os = "windows"))]
    {
        use std::sync::OnceLock;
        static AVAILABLE: OnceLock<bool> = OnceLock::new();
        if *AVAILABLE.get_or_init(|| which::which("pwsh").is_ok()) {
            "Execute a PowerShell (Core) command via `pwsh`. This is not Windows, so `bash` is the native shell here and is what you should reach for by default; use this only when a task genuinely calls for PowerShell. Streams live output to the GUI as it runs."
        } else {
            "UNAVAILABLE on this machine: PowerShell Core (`pwsh`) is not installed and this is not Windows. Use `bash` instead — it is the native shell here. Calling this tool will fail."
        }
    }
}

/// Documents the narration cursor, which is not control and is easy to
/// mistake for it. Appended on every platform: it is only a window.
const NARRATION_DOC: &str = "

SHOWING THE USER (point_at / highlight / clear_pointer) — draws a second, decorative cursor. It does NOT move the real mouse, click, or take focus. point_at{x,y,text} marks a point with a caption, highlight{x,y,width,height,text} rings a region, clear_pointer removes it. Use it when explaining rather than doing.";

/// The Windows-only half of the `dip` description — structured mode.
#[cfg(target_os = "windows")]
const STRUCTURED_MODE_DOC: &str = "

STRUCTURED MODE — Windows. PREFER THIS for any normal app: it is faster and far more reliable than pixels.
- list_windows: every open window, front to back, with ids.
- launch_app{command, args?}: start an app, document, folder or URI (notepad, calc, a file path, ms-settings:display, https://...). Waits for its window and makes it the target — you can type into it straight away.
- focus_window{id}: bring a window forward and make it the target. window_action{id, window_state}: minimize/maximize/restore/close.
- snapshot{id?}: the window's UI as an indented list, e.g. `[12] Edit \"File name\" = \"report.txt\" *focused`, read in one fast pass. Without an id it reads the target window.
- find{query}: only the elements matching a name, value or type — the quick way to one element in a big window.
- interact{id, element_action, value?}: act on an element by id. element_action: click, double_click, right_click, invoke, focus, set_value (REPLACES the field's text), toggle, select, expand, collapse, scroll_into_view.
- read_text{id?}: the full text of a document, field or the whole window.
Ids last until the next snapshot/find; after anything that changes the UI, snapshot again. Use pixel mode only for what snapshot can't see (games, canvas, custom-drawn UI).";

#[allow(unused_mut)] // `mut` on `computer_actions` only needed on Windows, extended below
pub(super) fn static_tool_definitions() -> Vec<serde_json::Value> {
    // The `computer` tool's action list — cross-platform "pixel mode" plus,
    // on Windows only, "structured mode" (real UI Automation tree instead of
    // guessed pixel coordinates; see `coding_agent::dip`). Built as a plain
    // Vec so the schema's `enum` only ever lists actions that actually
    // dispatch on this platform — see `computer::execute_computer_tool`.
    let mut computer_actions = vec![
        "screenshot", "zoom", "mouse_move", "left_click", "left_click_drag", "right_click",
        "middle_click", "double_click", "triple_click", "scroll", "type", "key", "cursor_position", "wait",
        // Narration, not control: these draw a marker on screen and move
        // nothing. Cross-platform, because they are only a Tauri window.
        "point_at", "highlight", "clear_pointer",
    ];
    #[cfg(target_os = "windows")]
    computer_actions.extend([
        "snapshot", "find", "read_text", "interact", "list_windows", "focus_window", "window_action", "launch_app",
        "hold_key", "key_down", "key_up",
    ]);

    // The description has to be gated the same way the `enum` above is. It
    // was not, so on macOS and Linux the model was told at length to prefer
    // structured mode, then rejected for calling an action the schema does
    // not list — an unforced error loop on every non-Windows host.
    #[cfg(target_os = "windows")]
    let structured_mode = STRUCTURED_MODE_DOC;
    #[cfg(not(target_os = "windows"))]
    let structured_mode = "\n\nSTRUCTURED MODE (reading an app's real UI tree instead of guessing pixels) is Windows-only and unavailable on this machine, so pixel mode is the only option here. Take a screenshot, act on what you see in it, then screenshot again to confirm the action landed.";

    let mut defs = vec![
        serde_json::json!({
            "name": "read",
            "description": "Read the contents of a file. Text files are returned line-numbered (like `cat -n`), with the total line count stated so you know whether you saw the whole file or need offset/limit to continue. Handles PDFs (page count + extracted text via 'pages' param) and Jupyter notebooks (.ipynb, cell-by-cell) the same way. Images are shown to you directly (downscaled if oversized) alongside a text summary (format/dimensions) — set ocr=true to also extract any text in the image instead of just seeing it. If the current model can't take image input, an image is OCR'd automatically instead of shown, so you still get any text it contains.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Relative path of the file" },
                    "offset": { "type": "number", "description": "Start reading at this line (1-indexed) — text files only" },
                    "limit": { "type": "number", "description": "Maximum lines to read — text files only" },
                    "pages": { "type": "string", "description": "PDF page range, e.g. \"1-5\" or \"3\" — PDF files only" },
                    "ocr": { "type": "boolean", "description": "If true, also run OCR on image files to extract literal text (requires HUGGINGFACE_TOKEN, with a local RapidOCR fallback)." }
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
            "description": "Execute a bash command in the workspace. Streams live output to the GUI as it runs. Output is labeled [stdout]/[stderr], the exit code is always appended at the end, and long output is truncated in the middle (kept: start + end) so a build/test failure's summary at the end is never cut off — the omitted middle is saved to a file whose path is in the truncation notice; read or grep it if you need something from the cut section.",
            "parameters": {
                "type": "object",
                "properties": {
                    "command": { "type": "string", "description": "Shell command to execute" },
                    "timeout": { "type": "number", "description": "Timeout in seconds (default 120, max 600)" }
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
            "name": "dip",
            "description": "Control the computer: apps, windows, mouse, keyboard and screen.\n\nPIXEL MODE — works on anything. screenshot{window?, monitor?} returns a scaled image; every coordinate you pass afterwards is in THAT image's pixels and is mapped to the screen for you. zoom{region:[x0,y0,x1,y1]} gives a closer look to read small text. Clicks: left_click / right_click / middle_click / double_click / triple_click with optional coordinate; mouse_move; left_click_drag{coordinate, start_coordinate?}; scroll{direction, amount, coordinate?}. Keyboard: type{text} (any text, emoji and newlines included); key{text:\"ctrl+s\", repeat?} — names like enter, tab, esc, win, alt, shift, f1-f24, up/down/left/right, home, end, pagedown, delete, volume_up, or a single character; hold_key{text, duration}; key_down / key_up. Typing and keys go to the TARGET window (the app you last launched, focused, snapshotted or clicked in) even while Vivianne is in front — you never type into the chat by accident. wait{duration} pauses for loading. cursor_position.".to_string() + structured_mode + NARRATION_DOC,
            "parameters": {
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": computer_actions,
                        "description": "Which action to perform."
                    },
                    "coordinate": {
                        "type": "array",
                        "items": { "type": "number" },
                        "description": "[x, y] in the last screenshot's pixels (screen pixels if you haven't taken one)."
                    },
                    "start_coordinate": { "type": "array", "items": { "type": "number" }, "description": "left_click_drag: where to start (default: the current cursor)." },
                    "text": { "type": "string", "description": "type: the text. key / hold_key / key_down / key_up: a key or combination like \"ctrl+shift+t\". point_at / highlight: a caption." },
                    "repeat": { "type": "number", "description": "key: press it this many times (e.g. down x5)." },
                    "duration": { "type": "number", "description": "Seconds — wait, or how long hold_key holds." },
                    "direction": { "type": "string", "enum": ["up", "down", "left", "right"], "description": "scroll direction." },
                    "amount": { "type": "number", "description": "scroll: wheel notches (default 3)." },
                    "region": { "type": "array", "items": { "type": "number" }, "description": "zoom: [x0, y0, x1, y1] in the last screenshot's pixels." },
                    "window": { "type": "boolean", "description": "screenshot: capture just the target window instead of the whole screen." },
                    "monitor": { "type": "number", "description": "screenshot: which monitor (0-based); default is the one the target window is on." },
                    "id": { "type": "number", "description": "interact / read_text: an element id from the last snapshot or find. snapshot / find / focus_window / window_action: a window id from list_windows." },
                    "query": { "type": "string", "description": "find: part of the element's name, text or type." },
                    "element_action": {
                        "type": "string",
                        "enum": ["click", "double_click", "right_click", "invoke", "focus", "set_value", "toggle", "select", "expand", "collapse", "scroll_into_view"],
                        "description": "interact: what to do (default click)."
                    },
                    "value": { "type": "string", "description": "interact with set_value: the text the field should contain." },
                    "window_state": {
                        "type": "string",
                        "enum": ["minimize", "maximize", "restore", "close"],
                        "description": "window_action: what to do to the window."
                    },
                    "command": { "type": "string", "description": "launch_app: an app name, path, document, folder or URI." },
                    "args": { "type": "array", "items": { "type": "string" }, "description": "launch_app: arguments." },
                    "highlight": { "type": "boolean", "description": "snapshot: also ring the window on screen for the user." },
                    "x": { "type": "number", "description": "point_at / highlight: x in screenshot pixels." },
                    "y": { "type": "number", "description": "point_at / highlight: y in screenshot pixels." },
                    "width": { "type": "number", "description": "highlight: width." },
                    "height": { "type": "number", "description": "highlight: height." }
                },
                "required": ["action"]
            }
        }),
        serde_json::json!({
            "name": "inspect_document",
            "description": "List the addressable parts of an existing PDF, Word, Excel or PowerPoint file, each with a stable id you can pass to edit_document. Call this FIRST whenever you need to change part of a document that already exists — the ids are positional, so guessing one edits the wrong thing. Part ids: docx \"p:3\" (4th paragraph), xlsx \"Sheet1!B4\" (one cell), pptx \"slide:2:1\" (2nd text box on slide 2), pdf \"page:5\". Each part reports whether it is editable — PDF pages are listed so you can read them but their text cannot be edited in place.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Path to the .pdf/.docx/.xlsx/.pptx file" }
                },
                "required": ["path"]
            }
        }),
        serde_json::json!({
            "name": "edit_document",
            "description": "Change one part of an EXISTING document in place, leaving the rest of the file byte-identical — styles, formulas, charts and images all survive. Use this instead of regenerating with create_document whenever the file already exists, because regenerating discards every change the user made by hand.

Always call inspect_document first to get the part id. Pass 'part' plus the replacement 'text'.

PDF: text CANNOT be edited — a PDF stores positioned glyphs rather than sentences, so there is nothing to rewrite. Page deletion works via 'delete_pages'; for new wording, regenerate with create_document.

XLSX edits existing cells only; use create_document to build a sheet with new ones.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Path to the existing document" },
                    "part": { "type": "string", "description": "Part id from inspect_document, e.g. \"p:3\", \"Sheet1!B4\", \"slide:2:1\"" },
                    "text": { "type": "string", "description": "Replacement text for that part" },
                    "delete_pages": { "type": "array", "items": { "type": "number" }, "description": "PDF only: page numbers to delete. Use instead of part/text." }
                },
                "required": ["path"]
            }
        }),
        serde_json::json!({
            "name": "create_document",
            "description": "Create a real PDF, Word (docx), Excel (xlsx), or PowerPoint (pptx) file on disk that opens in the corresponding app. Use this when the user asks for a document, report, spreadsheet, or deck as a FILE — not when they just want text in the chat.

pdf/docx take `blocks`: an ordered array of {type: \"heading\"|\"paragraph\"|\"bullets\"|\"table\"} where heading has `text` and `level` (1-3), paragraph has `text`, bullets has `items` (array of strings), and table has `rows` (array of arrays; the first row is treated as a header).
xlsx takes `sheets`: [{name, rows: [[cell, ...], ...]}] — numbers stay numeric so Excel can sum them, and the first row is bolded as a header.
pptx takes `slides`: [{title, bullets: [...]}] — title-and-bullets layout only.",
            "parameters": {
                "type": "object",
                "properties": {
                    "format": { "type": "string", "enum": ["pdf", "docx", "xlsx", "pptx"], "description": "Which file type to write" },
                    "path": { "type": "string", "description": "Where to write it, relative to the workspace (e.g. \"reports/q1.xlsx\")" },
                    "title": { "type": "string", "description": "Document title, rendered at the top — pdf/docx only" },
                    "blocks": { "type": "array", "items": { "type": "object" }, "description": "Ordered content blocks — required for pdf and docx" },
                    "sheets": { "type": "array", "items": { "type": "object" }, "description": "Worksheets — required for xlsx" },
                    "slides": { "type": "array", "items": { "type": "object" }, "description": "Slides — required for pptx" }
                },
                "required": ["format", "path"]
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
            "description": powershell_description(),
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
            "description": "Open a URL, or a local file the browser can render (HTML, PDF, SVG, images), in a live preview panel inside the app — a real screenshot from an actual browser (camoufox-browser), not a sandboxed iframe, so it works even for sites/pages that block being framed. Use web_fetch instead when you just need to read the content as text yourself. This only navigates and screenshots — for actually interacting with the page (browsing the web hands-on, or exercising a local dev build), use browser_snapshot/browser_click/browser_type/browser_scroll/browser_evaluate after opening it here. Requires camoufox-browser running (npx @askjo/camofox-browser) — if it isn't, this returns a clear error explaining how to start it.",
            "parameters": {
                "type": "object",
                "properties": {
                    "url": { "type": "string", "description": "http(s):// URL, file:// URL, or a bare local path like \"C:\\report.html\" or \"/tmp/output.pdf\" (auto-converted to file://)" }
                },
                "required": ["url"]
            }
        }),
        serde_json::json!({
            "name": "browser_snapshot",
            "description": "Get the accessibility tree (DOM structure, not a picture) of the page currently open via web_preview, with element refs (e1, e2, ...) that browser_click/browser_type target. Call this first to see what's on the page and find the ref of the element you want, and again after any action that could have changed the page (navigation, a click that opens/closes something, typing that triggers a live update) — refs from a stale snapshot can point at the wrong element or nothing at all. Large pages are windowed to keep this readable: if the result says 'truncated', call this again with the given offset to read further into the page rather than assuming you've seen everything.",
            "parameters": {
                "type": "object",
                "properties": {
                    "offset": { "type": "number", "description": "Character offset to resume from — only needed when a previous call reported 'truncated' and gave a next offset to continue from." }
                }
            }
        }),
        serde_json::json!({
            "name": "browser_click",
            "description": "Click the element with accessibility ref `ref` (from browser_snapshot) on the page currently open via web_preview.",
            "parameters": {
                "type": "object",
                "properties": {
                    "ref": { "type": "string", "description": "Element ref from browser_snapshot, e.g. \"e3\"" }
                },
                "required": ["ref"]
            }
        }),
        serde_json::json!({
            "name": "browser_type",
            "description": "Type text into the element with accessibility ref `ref` (from browser_snapshot) on the page currently open via web_preview.",
            "parameters": {
                "type": "object",
                "properties": {
                    "ref": { "type": "string", "description": "Element ref from browser_snapshot, e.g. \"e5\"" },
                    "text": { "type": "string", "description": "Text to type into the element" },
                    "submit": { "type": "boolean", "description": "Press Enter after typing (e.g. to submit a search box)", "default": false }
                },
                "required": ["ref", "text"]
            }
        }),
        serde_json::json!({
            "name": "browser_press_key",
            "description": "Press a single key or combo (e.g. \"Enter\", \"Escape\", \"Control+A\") at whatever currently has focus on the page open via web_preview.",
            "parameters": {
                "type": "object",
                "properties": {
                    "key": { "type": "string", "description": "Key or combo to press, e.g. \"Enter\" or \"Control+A\"" }
                },
                "required": ["key"]
            }
        }),
        serde_json::json!({
            "name": "browser_scroll",
            "description": "Scroll the page currently open via web_preview.",
            "parameters": {
                "type": "object",
                "properties": {
                    "direction": { "type": "string", "enum": ["up", "down", "left", "right"], "default": "down" },
                    "amount": { "type": "number", "description": "Pixels to scroll (default 500)" }
                }
            }
        }),
        serde_json::json!({
            "name": "browser_evaluate",
            "description": "Run arbitrary JavaScript in the page currently open via web_preview and return the result (must be JSON-serializable). For reading page state, computed values, or anything not covered by a dedicated tool. Prefer browser_read_console/browser_read_network for those specific cases.",
            "parameters": {
                "type": "object",
                "properties": {
                    "expression": { "type": "string", "description": "JavaScript expression to evaluate in the page, e.g. \"document.title\" or \"document.querySelectorAll('a').length\"" }
                },
                "required": ["expression"]
            }
        }),
        serde_json::json!({
            "name": "browser_read_console",
            "description": "Read console.log/warn/error output from the page currently open via web_preview. Best-effort: installs a capture shim on first call, so it only sees messages logged from that point forward — it cannot retroactively recover console output from before this was called. Draining is destructive (each call returns and clears the buffer since the last read).",
            "parameters": { "type": "object", "properties": {} }
        }),
        serde_json::json!({
            "name": "browser_read_network",
            "description": "Read network requests the page currently open via web_preview has made, via the Resource Timing API — URL, type, duration, transfer size for each. Best-effort and passive: no request/response headers or bodies (there's no DevTools protocol access here), and it only sees requests still in the browser's timing buffer (recent ones; a long time or heavy traffic since page load may have evicted earlier entries).",
            "parameters": { "type": "object", "properties": {} }
        }),
        serde_json::json!({
            "name": "browser_start",
            "description": "Start the browser and report its state. CALL THIS FIRST, before any other browser tool, at the start of a browsing task.

The other browser tools do start it themselves, so this is not required for them to work — but a cold start can mean an npm install, a native rebuild and a ~300MB engine download, and folded into a web_preview call that looks like a hang. Doing it as its own step makes the wait attributable, reports a failure once with the server's own reason attached, and tells you which sites you are already signed in to before you plan around a login wall.",
            "parameters": { "type": "object", "properties": {} }
        }),
        serde_json::json!({
            "name": "browser_session_status",
            "description": "List which sites the browser is currently signed in to. Call this BEFORE telling the user a site needs a login — the session persists across restarts, so you may already have access from a previous day.",
            "parameters": { "type": "object", "properties": {} }
        }),
        serde_json::json!({
            "name": "browser_import_cookies",
            "description": "Import a Netscape-format cookies.txt file to sign the browser in to a site. The user exports cookies from the browser they are already signed in with; you never handle their password. Once imported the sign-in persists across restarts.

When you hit a login wall: call browser_session_status first, and if there is genuinely no session, use `question` to ask the user to export cookies for that site into the cookies folder, then call this with the file name. Do NOT try to type credentials into the page, and do not offer to create an account.",
            "parameters": {
                "type": "object",
                "properties": {
                    "file": { "type": "string", "description": "File name inside ~/.camofox/cookies/, or an absolute path" }
                },
                "required": ["file"]
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
            "description": "Create and manage a structured task checklist for the session. Each completed item becomes a visible checkpoint of your progress, so plan real milestones, mark each completed (with a summary) as soon as it is done, and keep the list current. Replaces the whole list each call. At most one item may be 'in_progress' at a time, and content must be unique across items — both are rejected as errors.",
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
                                "status": { "type": "string", "enum": ["pending", "in_progress", "completed"] },
                                "summary": { "type": "string", "description": "When marking an item completed: one short line on what was actually done (e.g. 'added /login route and form, 4 tests pass'). Shown to the user as a checkpoint." }
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
            "description": "Generate a new image from a text description using an image-generation model. Defaults to OpenAI's gpt-image-2; pass provider=\"google\" to use Google's Imagen instead. Requires an API key for whichever provider is used, configured in Settings → API Keys — if none is set, this returns an error saying so. Use for creating original images (illustrations, mockups, icons), not for analysing an existing image (that's just done by looking at it, no tool needed).",
            "parameters": {
                "type": "object",
                "properties": {
                    "prompt": { "type": "string", "description": "Description of the image to generate" },
                    "size": { "type": "string", "description": "OpenAI only. Image size, e.g. \"1024x1024\", \"1536x1024\", \"1024x1536\" (default \"1024x1024\")" },
                    "provider": { "type": "string", "enum": ["openai", "google"], "description": "Which provider to generate with (default \"openai\")" },
                    "model": { "type": "string", "description": "Override the default model for the chosen provider, e.g. \"gpt-image-1-mini\" (cheaper) for openai, or \"imagen-4.0-ultra-generate-001\" for google" }
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
            "name": "set_panel",
            "description": "Open or close a GUI panel, so the user can actually see what you're doing (e.g. open Review while you work through a diff, or the terminal panel while you run a long command) instead of it only ever reacting to clicks. Purely advisory — always succeeds regardless of whether the panel is currently visible on screen.",
            "parameters": {
                "type": "object",
                "properties": {
                    "panel": { "type": "string", "description": "One of: review (diff/review panel), files (workspace file tree), preview_dev (the Preview panel in dev-server mode), preview_browser (the Preview panel in browser mode — web_preview/browser_* activity), documents (the Preview panel in Documents mode — where the user views and edits pdf/docx/xlsx/pptx files; open it when working on a document with them), tasks (background subagent tasks), plan (the Plan panel — plans under review or being carried out), terminal (the bottom terminal panel). Or \"custom:<name>\" for a panel-type playbook (see list_tools for what's installed — Settings → Mods → Playbooks)." },
                    "action": { "type": "string", "enum": ["open", "close"] }
                },
                "required": ["panel", "action"]
            }
        }),
        serde_json::json!({
            "name": "get_goal",
            "description": "Get this session's current goal (the durable objective you're working toward), if one is set. Distinct from todo_write's checklist — the goal is the point of the steps, not the steps themselves.",
            "parameters": { "type": "object", "properties": {} }
        }),
        serde_json::json!({
            "name": "create_goal",
            "description": "Set the goal of the task you were given, replacing any existing one. Do this for any task that takes more than a few steps. While the goal is active you keep working on it — ending a turn does not end the task; you will be prompted to continue — through as many steps, hours or days as it takes, until you call update_goal with action 'complete' (done and verified) or 'blocked' (you need the user). Rate limits, model outages and app restarts are handled for you. Not for quick questions.",
            "parameters": {
                "type": "object",
                "properties": {
                    "objective": { "type": "string", "description": "What you're trying to accomplish, stated concretely" }
                },
                "required": ["objective"]
            }
        }),
        serde_json::json!({
            "name": "update_goal",
            "description": "Update the current goal's phase or wording. Fails if no goal is set — call create_goal first.",
            "parameters": {
                "type": "object",
                "properties": {
                    "action": { "type": "string", "enum": ["edit", "pause", "resume", "complete", "blocked"] },
                    "objective": { "type": "string", "description": "New wording — action='edit' only" },
                    "blocked_reason": { "type": "string", "description": "Why it's blocked — action='blocked' only" }
                },
                "required": ["action"]
            }
        }),
        serde_json::json!({
            "name": "load_tools",
            "description": "Add tools to your function list. To keep you fast, only core tools are loaded at first; your system prompt names every group you have. If a tool you need isn't in your function list, call this with its group, and it is callable from your next step. Loaded tools stay loaded for the rest of the conversation.",
            "parameters": {
                "type": "object",
                "properties": {
                    "groups": { "type": "array", "items": { "type": "string" }, "description": "Group names from your capability list, e.g. [\"GitHub\"] or [\"browse\"]. \"custom\" loads MCP and playbook tools." },
                    "tools": { "type": "array", "items": { "type": "string" }, "description": "Exact tool names, when you know them." }
                }
            }
        }),
        serde_json::json!({
            "name": "list_tools",
            "description": "List every tool currently registered — built-in, MCP-provided, and playbook-defined — with whether each is enabled or disabled right now. Use this to check what's actually available before assuming a tool exists or is reachable, e.g. after connecting an MCP server or when a call unexpectedly fails as disabled.",
            "parameters": { "type": "object", "properties": {} }
        }),
        serde_json::json!({
            "name": "terminal_open",
            "description": "Open a persistent, real shell session (same kind the user's terminal tabs use) and return its id. Unlike bash/powershell, this survives across calls — use it for interactive programs (a REPL, an installer with prompts), or work that needs shell state kept alive between steps (an activated venv, an ssh session, a long dev-server process you'll poll). For a single one-shot command, prefer bash/powershell — they're simpler and don't need cleanup.",
            "parameters": {
                "type": "object",
                "properties": {
                    "shell_id": { "type": "string", "description": "Which shell to launch (see terminal_list output or omit for the platform default: powershell on Windows, bash elsewhere)" },
                    "cwd": { "type": "string", "description": "Working directory to start in (default: the workspace root)" }
                }
            }
        }),
        serde_json::json!({
            "name": "terminal_send",
            "description": "Type a line into an open terminal session and press Enter (a trailing newline is added if missing). Doesn't wait for or return output — call terminal_read afterward to see the result.",
            "parameters": {
                "type": "object",
                "properties": {
                    "tab_id": { "type": "string", "description": "Session id returned by terminal_open" },
                    "data": { "type": "string", "description": "Text to type" }
                },
                "required": ["tab_id", "data"]
            }
        }),
        serde_json::json!({
            "name": "terminal_read",
            "description": "Read everything a terminal session has printed since the last terminal_read call on it (or since it opened, on the first call). Returns '(no new output since the last read)' if nothing new arrived — that does not mean the command finished; a foreground process can still be running with no new output yet.",
            "parameters": {
                "type": "object",
                "properties": {
                    "tab_id": { "type": "string", "description": "Session id returned by terminal_open" }
                },
                "required": ["tab_id"]
            }
        }),
        serde_json::json!({
            "name": "terminal_signal",
            "description": "Send Ctrl-C to whatever's running in the foreground of a terminal session, stopping it without closing the session. To end the session entirely, use terminal_close.",
            "parameters": {
                "type": "object",
                "properties": {
                    "tab_id": { "type": "string", "description": "Session id returned by terminal_open" }
                },
                "required": ["tab_id"]
            }
        }),
        serde_json::json!({
            "name": "terminal_close",
            "description": "Close a terminal session and terminate its shell process. Call this when you're done with a session you opened — it doesn't clean itself up.",
            "parameters": {
                "type": "object",
                "properties": {
                    "tab_id": { "type": "string", "description": "Session id returned by terminal_open" }
                },
                "required": ["tab_id"]
            }
        }),
        serde_json::json!({
            "name": "terminal_list",
            "description": "List every open terminal session (yours and the user's own terminal tabs), with shell, working directory, and when it was opened.",
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
            "description": "Launch a subagent. By default (run_in_background=true) this returns a task_id immediately — poll or block on it with task_output/wait_tasks — instead of blocking your own turn, and it shows up live in the Background Tasks panel so the user can see it running. Call this once per subagent — spawn multiple concurrently rather than looping serial blocking calls, and keep making progress yourself (other tool calls, or delegating more independent chunks) instead of idling on task_output while one runs. Set run_in_background=false only for a single one-off subtask where you have nothing else to do until it returns. Delegation is at most three tiers: you, then the agents you spawn; a project employee may spawn plain subagents of its own, and those cannot delegate further. Call list_subagent_profiles first if you're not sure whether a profile with a preset prompt already exists for this — pointing at one (subagent_type + a short context) costs far fewer tokens than writing the full prompt yourself.",
            "parameters": {
                "type": "object",
                "properties": {
                    "description": { "type": "string", "description": "Short (3-5 word) description of the task" },
                    "prompt": { "type": "string", "description": "Self-contained instructions for the subagent — it does not see this conversation. Required unless subagent_type names a profile with a preset default_prompt (check with list_subagent_profiles) — in that case omit this and use context instead." },
                    "subagent_type": { "type": "string", "description": "Label for the subagent's role (default 'general-purpose')", "default": "general-purpose" },
                    "context": { "type": "string", "description": "A short pointer (a file path, a target, a one-line description) appended to the matched profile's preset default_prompt. Only meaningful when prompt is omitted and subagent_type names a profile with default_prompt set — check list_subagent_profiles first." },
                    "persona": { "type": "string", "enum": ["security-auditor", "senior-dev", "debugger"], "description": "Optional role framing layered into the subagent's own directive — pick one when the task genuinely calls for that lens (a security review, an architecture-minded implementation, a bug hunt with a fixed reproduce/isolate/fix/verify order). Omit for general-purpose work." },
                    "run_in_background": { "type": "boolean", "description": "If true (default), return a task_id immediately instead of blocking", "default": true }
                },
                "required": ["description"]
            }
        }),
        serde_json::json!({
            "name": "hire_agent",
            "description": "Propose a specialist agent for this project's team. It is saved as a proposal in .viv/agents/ and the user approves or declines it; once approved you delegate to it with task (subagent_type = its id). Hire for a lasting area of responsibility (e.g. frontend, API, tests), not a one-off task. Only works in a project with a master agent.",
            "parameters": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Short display name, e.g. 'Frontend Engineer'" },
                    "role": { "type": "string", "description": "One sentence: what this agent is responsible for" },
                    "instructions": { "type": "string", "description": "Standing instructions it receives on every task: conventions, the charter decisions that apply to its area, how to verify its work" },
                    "reason": { "type": "string", "description": "Why the project needs this hire now — shown to the user when they decide" },
                    "owns": { "type": "array", "items": { "type": "string" }, "description": "Project-relative paths it is responsible for, e.g. ['src/ui/', 'styles/']. Keep specialists' paths from overlapping." },
                    "allowed_tools": { "type": "array", "items": { "type": "string" }, "description": "Optional tool allowlist; omit for full access" },
                    "reports_to": { "type": "string", "description": "Id of the agent it reports to; omit to report to the master" }
                },
                "required": ["name", "role", "instructions", "reason"]
            }
        }),
        serde_json::json!({
            "name": "list_subagent_profiles",
            "description": "List every subagent profile (built-in and custom) available to task, each with its id, description, allowed_tools restriction, and whether it has a preset default_prompt. Call this before task when you're not sure whether a profile already exists for what you're about to do — a profile with default_prompt set can be invoked with just subagent_type + a short context instead of writing a full prompt.",
            "parameters": { "type": "object", "properties": {} }
        }),
        serde_json::json!({
            "name": "task_output",
            "description": "Poll (or wait for) one or more background subagents launched with task. Returns a status snapshot per id: running, completed (with output), failed, or cancelled. With timeout_ms set: mode='any' (default) blocks until at least one id is terminal; mode='all' blocks until every id is terminal. Use this instead of a polling loop.",
            "parameters": {
                "type": "object",
                "properties": {
                    "task_ids": { "type": "array", "items": { "type": "string" }, "description": "Subagent ids returned by task" },
                    "timeout_ms": { "type": "number", "description": "If set, block up to this many milliseconds waiting on mode's condition (default 0 = no wait, just a snapshot)" },
                    "mode": { "type": "string", "enum": ["any", "all"], "default": "any", "description": "Only matters when timeout_ms > 0: 'any' returns as soon as one id is terminal, 'all' waits for every id" }
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
            "name": "team_run",
            "description": "Give a task to this project's team of agents (the ones the user built in the Agents panel). Every chosen member works on it at the same time with its own prompt, model and tools, and they message each other as they go. Returns when all have finished, with each member's report and what they said to each other, for you to combine into one answer.",
            "parameters": {
                "type": "object",
                "properties": {
                    "task": { "type": "string", "description": "The task, complete and self-contained — members don't see this conversation" },
                    "agents": { "type": "array", "items": { "type": "string" }, "description": "Member ids or names to involve; omit for the whole team" }
                },
                "required": ["task"]
            }
        }),
        serde_json::json!({
            "name": "message_agent",
            "description": "Send a message to a teammate in this team run, or to everyone. It reaches them between their steps. Use it to split work, share what another member needs, ask something only they know, or flag a conflict.",
            "parameters": {
                "type": "object",
                "properties": {
                    "to": { "type": "string", "description": "A teammate's id or name, or \"all\"" },
                    "text": { "type": "string" }
                },
                "required": ["to", "text"]
            }
        }),
        serde_json::json!({
            "name": "submit_plan",
            "description": "Submit a plan for the user to review — the end of every plan-mode turn, and how you revise a plan after feedback. The plan opens in the user's Plan panel, where they comment on individual sections and steps, then ask for changes or approve. Write it for a reader deciding whether to approve: concrete, specific to this project, grounded in what you found. Sections should cover the goal, what you found (the relevant code, the dependencies involved with their current versions, and anything learned from the web, with sources), the approach, risks and open questions, and how the result will be verified. Steps are the units of work the team will carry out, each sized for one specialist, with dependencies where one needs another's result. To revise, submit the whole plan again with the same plan_id and say what changed in 'changes'.",
            "parameters": {
                "type": "object",
                "properties": {
                    "plan_id": { "type": "string", "description": "Only when revising: the id of the plan being revised. Omit for a new plan." },
                    "title": { "type": "string", "description": "Short name of the plan" },
                    "summary": { "type": "string", "description": "Two or three sentences: what will be done and why" },
                    "sections": {
                        "type": "array",
                        "description": "The body of the plan, in reading order",
                        "items": {
                            "type": "object",
                            "properties": {
                                "id": { "type": "string", "description": "Stable id, kept across revisions (e.g. 'findings') — comments attach to it" },
                                "heading": { "type": "string" },
                                "body": { "type": "string", "description": "Markdown" }
                            },
                            "required": ["id", "heading", "body"]
                        }
                    },
                    "steps": {
                        "type": "array",
                        "description": "The work, in order",
                        "items": {
                            "type": "object",
                            "properties": {
                                "id": { "type": "string", "description": "Stable id, kept across revisions (e.g. 'db-schema')" },
                                "title": { "type": "string" },
                                "detail": { "type": "string", "description": "What exactly to do, which files, and how to verify it" },
                                "owner": { "type": "string", "description": "Suggested team member (agent id), if the project has a team" },
                                "depends_on": { "type": "array", "items": { "type": "string" }, "description": "Ids of steps that must finish first" }
                            },
                            "required": ["id", "title", "detail"]
                        }
                    },
                    "changes": { "type": "string", "description": "When revising: what changed from the previous version and how it answers the feedback" }
                },
                "required": ["title", "summary", "sections", "steps"]
            }
        }),
        serde_json::json!({
            "name": "assign_plan_steps",
            "description": "Carry out a plan the user approved: give each step to a member of your team, and the steps run — independent ones in parallel, each only once the steps it depends on are done, each step's agent getting the reports of the steps it builds on. Returns when every step has finished, with each step's report, for you to review, integrate and verify. Steps without an assignment keep the owner the plan suggested, or go to a general-purpose agent. Calling it again on a plan that partly failed re-runs only the unfinished steps.",
            "parameters": {
                "type": "object",
                "properties": {
                    "plan_id": { "type": "string" },
                    "assignments": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "step_id": { "type": "string" },
                                "agent_id": { "type": "string", "description": "An approved team member's id, or \"general-purpose\"" }
                            },
                            "required": ["step_id", "agent_id"]
                        }
                    }
                },
                "required": ["plan_id"]
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
        }),
        crate::agent::math::schema(),
        crate::helium::schema(),
        serde_json::json!({
            "name": "create_tool",
            "description": "Write a new tool in Python and save it for reuse. Use it when a task needs something the other tools don't do and it's likely to come up again (a format conversion, a query against an API, an analysis the user keeps asking for). The saved tool joins the tool list of every model the user runs, in every project, from your next step. The code is a Python module defining `def run(**args)` that returns a string or anything JSON-serialisable; the standard library and whatever is pip-installed are available, and it runs in the project folder. Give `test_args` to run it once before saving: if that run fails, nothing is saved and you get the traceback. Before creating one, check your tool list for an existing tool that already does it.",
            "parameters": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "snake_case, 3-48 characters, saying what it does (e.g. \"csv_to_parquet\")." },
                    "description": { "type": "string", "description": "What it does, its inputs and output, and when to use it. Other models choose the tool by this text alone." },
                    "parameters": { "type": "object", "description": "JSON schema for its arguments: {\"type\": \"object\", \"properties\": {...}, \"required\": [...]}. The keys become run()'s keyword arguments." },
                    "code": { "type": "string", "description": "The Python module. Must define run(**args)." },
                    "test_args": { "type": "object", "description": "Arguments for a test run before saving. Strongly recommended." },
                    "replace": { "type": "boolean", "description": "Update an existing tool of this name (to fix or improve it)." },
                    "timeout_secs": { "type": "integer", "description": "How long one call may run (default 120, at most 600)." }
                },
                "required": ["name", "description", "parameters", "code"]
            }
        }),
        serde_json::json!({
            "name": "delete_tool",
            "description": "Retire a custom tool you or another model created, when it is unused for a long time, duplicated by another tool, or superseded. It leaves every tool list; its files are kept in ~/.viv/tools/.retired/.",
            "parameters": {
                "type": "object",
                "properties": {
                    "name": { "type": "string" },
                    "reason": { "type": "string", "description": "Why, in a sentence. The user sees it." }
                },
                "required": ["name", "reason"]
            }
        }),
        serde_json::json!({
            "name": "list_custom_tools",
            "description": "List the custom tools (the ones created with create_tool) with how often and how recently each was used, and when it was made. For reviewing the set.",
            "parameters": { "type": "object", "properties": {} }
        }),
        serde_json::json!({
            "name": "run_script",
            "description": "Run a short JavaScript script that calls your own tools directly, in one turn, instead of one tool call per round-trip — chain several reads/searches before a write, loop over a list of files, fan out web searches, or post-process results before you see them. EVERY tool you have is callable from the script (all async — use await; each returns the tool's text output):\n  callTool(name, args) — any tool by name with its normal JSON arguments, e.g. callTool(\"web_search\", {query: \"...\"})\n  tools.<name>(args) — same thing, e.g. tools.web_fetch({url})\n  <name>(args) — tools whose names are plain identifiers are also globals, e.g. await todo_write({todos})\n  listTools() — the names available\nShorthands with positional arguments:\n  bash(command, timeoutSeconds?)  read(path, opts?)  write(path, content)  edit(path, oldString, newString)  ls(path?, opts?)  grep(pattern, path?)  glob(pattern, path?)\nconsole.log output is returned above the script's return value. Each call goes through the same approval/gating as calling the tool directly. A failed call throws a catchable JS exception. A script cannot start another run_script. Use this for genuine multi-step work; call a tool directly for a single step.",
            "parameters": {
                "type": "object",
                "properties": {
                    "code": { "type": "string", "description": "JavaScript source to execute. May use top-level await." },
                    "timeout_seconds": { "type": "number", "description": "Max seconds before the script is aborted (default 30, max 120)" }
                },
                "required": ["code"]
            }
        }),
    ];

    let mut defs = defs;
    defs.extend(crate::coding_agent::github::schemas());
    defs.push(crate::multiplayer::collab::pair_activity_schema());
    defs
}

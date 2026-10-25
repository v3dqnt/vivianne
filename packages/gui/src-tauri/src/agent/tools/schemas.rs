//! Static tool schema definitions (JSON parameter specs) sent to the model.
//! Pure data, no dispatch logic — split out of tool_exec.rs (which mixes
//! schemas, the execution loop, and per-tool handlers) so the two concerns
//! don't have to be scrolled through together.

/// Every top-level tool name a mod's tool must NOT be allowed to take:
/// every built-in tool's own name, plus `"computer"` — not itself a schema
/// entry, but a name `agent::tools::exec` still dispatches as a silent alias
/// for `dip` (see its match arm), so it needs to be off-limits too.
///
/// Mod manifests are validated against it (`mods::manifest`), so a mod
/// can't add a tool that would duplicate a schema entry or never run
/// because the built-in of the same name is found first.
///
/// Built once: it's checked on every custom-tool scan, which a local model's
/// request runs each turn.
pub(crate) fn reserved_tool_names() -> std::collections::HashSet<String> {
    static NAMES: std::sync::OnceLock<std::collections::HashSet<String>> = std::sync::OnceLock::new();
    NAMES
        .get_or_init(|| {
            let mut names: std::collections::HashSet<String> =
                static_tool_definitions().iter().filter_map(|d| d.get("name").and_then(|v| v.as_str())).map(String::from).collect();
            names.insert("computer".to_string());
            names
        })
        .clone()
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
        "Execute a PowerShell command. Preferred on Windows for shell operations."
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

/// Structured mode as both platforms have it. The per-action arguments and
/// enums live in the schema's parameters; this is only what they can't say.
/// A macro because `concat!` takes literals.
macro_rules! structured_common {
    () => {
        "
- list_windows, launch_app{command}, focus_window{id}, window_action{id, window_state}: find, start and switch windows. launch_app waits for the window and makes it the target, so you can type into it straight away.
- snapshot{id?}: the window's UI as an indented list with element ids, e.g. `[12] Edit \"File name\" = \"report.txt\" *focused`, read in one fast pass. find{query}: only the matching elements — the quick way to one element in a big window.
- interact{id, element_action, value?}: act on an element. set_value REPLACES the field's text.
- read_text{id?}: the full text of a document, field or the whole window.
Ids last until the next snapshot/find; snapshot again after anything that changes the UI. Use pixel mode only for what snapshot can't see (games, canvas, custom-drawn UI)."
    };
}

/// The Windows-only half of the `dip` description — structured mode.
#[cfg(target_os = "windows")]
const STRUCTURED_MODE_DOC: &str = concat!(
    "

STRUCTURED MODE — Windows. PREFER THIS for any normal app: it is faster and far more reliable than pixels. launch_app takes an app, document, folder or URI (notepad, a file path, ms-settings:display, https://...).",
    structured_common!()
);

/// The macOS half of the `dip` description — structured mode through the
/// Accessibility API (`coding_agent::dip_mac`).
#[cfg(target_os = "macos")]
const STRUCTURED_MODE_DOC: &str = concat!(
    "

STRUCTURED MODE — macOS (Accessibility). PREFER THIS for any normal app: it is faster and far more reliable than pixels. It needs Vivianne allowed under System Settings → Privacy & Security → Accessibility; if a call says it isn't, tell the user once and use pixel mode meanwhile. launch_app takes an app name (Safari, \"Visual Studio Code\"), a file, a folder or a URL. Shortcuts use cmd, not ctrl: key{text: \"cmd+s\"}. element_action also takes increment and decrement.",
    structured_common!()
);

#[allow(unused_mut)] // `mut` on `computer_actions` only needed on Windows and macOS, extended below
pub(crate) fn static_tool_definitions() -> Vec<serde_json::Value> {
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
    #[cfg(target_os = "macos")]
    computer_actions.extend(["snapshot", "find", "read_text", "interact", "list_windows", "focus_window", "window_action", "launch_app"]);

    #[cfg(any(target_os = "windows", target_os = "macos"))]
    let structured_mode = STRUCTURED_MODE_DOC;
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let structured_mode = "\n\nSTRUCTURED MODE (reading an app's real UI tree instead of guessing pixels) is available on Windows and macOS but not on this machine, so pixel mode is the only option here. Take a screenshot, act on what you see in it, then screenshot again to confirm the action landed.";

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
            "description": "Execute a bash command in the workspace. Output is labeled [stdout]/[stderr] and ends with the exit code. Long output keeps its start and end; the omitted middle is saved to a file named in the notice — read or grep it if you need that part.",
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
            "description": "List a directory; set depth for a tree.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Relative path to list" },
                    "depth": { "type": "number", "description": "Tree depth, gitignore-aware (default 1 = just this folder, max 6)" },
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
            "description": "Control the computer: apps, windows, mouse, keyboard and screen.\n\nPIXEL MODE — works on anything. screenshot returns a scaled image; every coordinate afterwards is in THAT image's pixels and is mapped to the screen for you. zoom{region} gives a closer look to read small text. type{text} types anything, emoji and newlines included; key names: enter, tab, esc, win, alt, shift, f1-f24, up/down/left/right, home, end, pagedown, delete, volume_up, or a single character. Typing and keys go to the TARGET window (the app you last launched, focused, snapshotted or clicked in) even while Vivianne is in front — never into the chat. wait{duration} pauses for loading.".to_string() + structured_mode + NARRATION_DOC,
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
            "description": "Create a real, designed file: PDF, Word (docx), Excel (xlsx), PowerPoint (pptx), or an image (png). Use it whenever the user wants a document, deck, spreadsheet, poster or graphic as a FILE. Before making one, read the design guide for it with `guide` (`design/principles`, then `design/formats/<pdf|slides|docx|xlsx|graphics>`). After, look at the preview image attached to the result and fix anything that looks wrong.\n\
pdf/docx: `blocks` (heading, paragraph, bullets, numbered, table, image, quote, callout, code, metrics, chart, divider, page_break; inline **bold**, *italic*, `code`, [link](url)) with `title`, `subtitle`, `eyebrow`, `author`, `date`, `cover`. pdf can instead take `html` for any design the blocks can't express (a fragment gets the house stylesheet; a full document is used as is), or `slides` for a 16:9 PDF deck.\n\
pptx: `slides` [{title, subtitle?, bullets?, left/right {heading, bullets}, stats [{label, value, note}], chart, table, image, quote, by, layout?}] — layouts title, section, bullets, two_column, image, quote, stats, table, chart, closing.\n\
xlsx: `sheets` [{name, rows, columns [{width, format: currency|percent|number|integer|date}], total_row, charts [{type, title, categories: \"A\", values: [\"B\"], anchor}]}]; strings starting with = are formulas.\n\
png: `html` and `size` {width, height} in pixels: posters, social images, covers, cards.\n\
`theme`: modern (default), editorial, classic, minimal, warm, forest, midnight (dark) — or {preset, accent: \"#hex\"} for a brand colour.",
            "parameters": {
                "type": "object",
                "properties": {
                    "format": { "type": "string", "enum": ["pdf", "docx", "xlsx", "pptx", "png"] },
                    "path": { "type": "string", "description": "Where to write it, relative to the workspace. Name it for the reader: \"Q4 Board Report.pdf\"." },
                    "theme": { "description": "A preset name, or {preset, accent, accent2, heading_font, body_font}." },
                    "title": { "type": "string" },
                    "subtitle": { "type": "string" },
                    "eyebrow": { "type": "string", "description": "A short label above the title, e.g. \"Board report\"." },
                    "author": { "type": "string" },
                    "date": { "type": "string" },
                    "cover": { "type": "boolean", "description": "pdf/docx: a cover page." },
                    "footer": { "type": "string", "description": "Running head text (defaults to the title)." },
                    "page_size": { "type": "string", "enum": ["A4", "letter", "legal", "A3", "A5"], "description": "pdf/docx page (default A4)." },
                    "page": { "type": "object", "description": "pdf/docx custom page: {width_mm, height_mm}." },
                    "orientation": { "type": "string", "enum": ["portrait", "landscape"] },
                    "blocks": { "type": "array", "items": { "type": "object" } },
                    "html": { "type": "string", "description": "pdf/png: your own HTML/CSS. Fonts 'Inter' and 'Source Serif 4' are embedded." },
                    "size": { "type": "object", "description": "png: {width, height} in CSS pixels (rendered at 2x)." },
                    "slides": { "type": "array", "items": { "type": "object" } },
                    "sheets": { "type": "array", "items": { "type": "object" } },
                    "preview": { "type": "boolean", "description": "Attach a picture of the first page/slide (default true)." }
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
                    "query": { "type": "string", "description": "What to search memory for" },
                    "as_of": { "type": "string", "description": "Optional date (YYYY-MM-DD, or YYYY-MM): recall what was true then instead of now. Facts that were later corrected are kept on record, so this answers \"what did we decide in March\"." }
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
            "description": "Read console.log/warn/error output from the web_preview page. Only sees messages logged after its first call on that page, and each call returns and clears what arrived since the last.",
            "parameters": { "type": "object", "properties": {} }
        }),
        serde_json::json!({
            "name": "browser_read_network",
            "description": "List the web_preview page's recent network requests: URL, type, duration, transfer size. No headers or bodies; very old requests may have dropped out.",
            "parameters": { "type": "object", "properties": {} }
        }),
        serde_json::json!({
            "name": "browser_start",
            "description": "Start the browser and report its state and the sites it's signed in to. Call it first in a browsing task: a cold start can take minutes (install and a ~300MB engine download), and as its own step that wait doesn't look like a hung web_preview.",
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
            "description": "Ask the user and wait for the answer. Either 1-4 multiple-choice questions, for genuine decision points rather than to avoid a reasonable judgment call; or `confirm`, one yes/no check before an irreversible or consequential action the user hasn't clearly authorized (deleting real data, sending something externally visible), even when the tool itself wouldn't ask.",
            "parameters": {
                "type": "object",
                "properties": {
                    "confirm": { "type": "string", "description": "Instead of questions: the action to confirm, as a yes/no question, e.g. \"Delete all 40 rows matching this filter?\"" },
                    "danger_level": { "type": "string", "enum": ["low", "medium", "high"], "description": "With confirm: how hard to reverse (default medium)" },
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
                "required": []
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
                    "panel": { "type": "string", "description": "One of: review (diff/review panel), files (workspace file tree), preview_dev (the Preview panel in dev-server mode), preview_browser (the Preview panel in browser mode — web_preview/browser_* activity), documents (the Preview panel in Documents mode — where the user views and edits pdf/docx/xlsx/pptx files; open it when working on a document with them), tasks (background subagent tasks), plan (the Plan panel — plans under review or being carried out), terminal (the bottom terminal panel). Or \"mod:<key>/<panel id>\" for a panel a mod adds (Settings → Mods lists them)." },
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
            "description": "Add tools to your function list. To keep requests small, some tools are held back until needed; your system prompt names every group you have. If a tool you need isn't in your function list, call this with its group, and it is callable from your next step. Loaded tools stay loaded for the rest of the conversation.",
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
            "name": "terminal",
            "description": "A persistent shell that stays alive across calls, shared with the user's terminal panel — for when state matters (an interactive prompt, a long-running server, an activated venv); bash/powershell are for one-shot commands. open returns a session id; send types a line and presses Enter without waiting; read returns what printed since the last read (no new output doesn't mean the command finished); interrupt sends Ctrl-C; close ends the session (close the ones you open); list shows every session, the user's included.",
            "parameters": {
                "type": "object",
                "properties": {
                    "action": { "type": "string", "enum": ["open", "send", "read", "interrupt", "close", "list"] },
                    "tab_id": { "type": "string", "description": "Session id (all but open and list)" },
                    "data": { "type": "string", "description": "send: the line to type" },
                    "shell_id": { "type": "string", "description": "open: which shell (default: powershell on Windows, bash elsewhere)" },
                    "cwd": { "type": "string", "description": "open: starting folder (default: the workspace root)" }
                },
                "required": ["action"]
            }
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
            "description": "Launch a subagent. It returns a task_id at once and runs in the background (visible in the Background Tasks panel); collect it with task_output. Spawn independent subagents together rather than one after another, and keep working yourself instead of idling on task_output. run_in_background=false only for a single subtask you must wait on. Delegation is at most three tiers: you, the agents you spawn, and plain subagents a project employee spawns. If list_subagent_profiles has a profile with a preset prompt for this, pass its subagent_type and a short context instead of writing a prompt.",
            "parameters": {
                "type": "object",
                "properties": {
                    "description": { "type": "string", "description": "Short (3-5 word) description of the task" },
                    "prompt": { "type": "string", "description": "Self-contained instructions — the subagent doesn't see this conversation. Omit only for a profile with a preset prompt." },
                    "subagent_type": { "type": "string", "description": "The subagent's role, or a profile id", "default": "general-purpose" },
                    "context": { "type": "string", "description": "With a preset-prompt profile instead of prompt: a short pointer (a path, a target) appended to it." },
                    "run_in_background": { "type": "boolean", "description": "false to wait for the result", "default": true }
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
            "description": "List the subagent profiles task can use: id, description, allowed tools, and whether it has a preset prompt.",
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
                    "task": { "type": "string", "description": "The overall task, complete and self-contained — members don't see this conversation" },
                    "agents": { "type": "array", "items": { "type": "string" }, "description": "Member ids or names to involve, all on the whole task; omit for the whole team. Ignored when assignments is given." },
                    "assignments": {
                        "type": "array",
                        "description": "Split the task: one entry per member that has a part. Prefer this over agents.",
                        "items": {
                            "type": "object",
                            "properties": {
                                "agent": { "type": "string", "description": "Member id or name" },
                                "task": { "type": "string", "description": "This member's own part, self-contained" },
                                "needs": { "type": "array", "items": { "type": "string" }, "description": "Ids of members in this run that must finish first; their reports are handed to this one" }
                            },
                            "required": ["agent", "task"]
                        }
                    },
                    "timeout_secs": { "type": "number", "description": "Stop any member still working after this long (default 1200)" }
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
            "name": "team_post",
            "description": "Put something your teammates depend on on the team run's shared ledger: a decision you made, an interface you expose (names, signatures, endpoints), or a blocker. Members that start later are given the ledger, and the coordinator sees all of it. Keep each entry short and concrete.",
            "parameters": {
                "type": "object",
                "properties": {
                    "kind": { "type": "string", "enum": ["decision", "interface", "blocker", "note"] },
                    "text": { "type": "string" }
                },
                "required": ["kind", "text"]
            }
        }),
        serde_json::json!({
            "name": "team_read",
            "description": "Read the team run's shared ledger and see which members are working, done or waiting.",
            "parameters": { "type": "object", "properties": {} }
        }),
        serde_json::json!({
            "name": "submit_plan",
            "description": "Submit a plan for the user to review in the Plan panel — how every plan-mode turn ends. Write for a reader deciding whether to approve: concrete, specific to this project, grounded in what you found. Steps are units of work sized for one specialist, with depends_on where one needs another's result. To revise after feedback, resubmit the whole plan with the same plan_id and say what changed in 'changes'.",
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
            "description": "Run a plan the user approved: assign steps to team members (unassigned steps go to the plan's suggested owner, else general-purpose). Independent steps run in parallel, each after its depends_on, with those steps' reports. Returns every step's report for you to review, integrate and verify. Calling it again re-runs only unfinished steps.",
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
        crate::agent::tools::math::schema(),
        crate::helium::schema(),
        crate::vault::schema(),
        crate::nebius::schema(),
        // Shown only to a bot with its own address or number (`compute_restrictions`).
        crate::agent::team::contact::schemas()[0].clone(),
        crate::agent::team::contact::schemas()[1].clone(),
        crate::agent::tools::guides::schema(),
        crate::agent::tools::diagram_check::schema(),
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
            "description": "Run a short JavaScript program that calls your tools directly: one turn instead of one round-trip per call, and only what the program returns or logs reaches you — tool results it reads stay out of your context. Worth it for fan-out over many items, results to filter or aggregate before you see them (search, logs, many files), and loops. Not worth it for one call, or steps where each depends on your judgment of the last. Calls with plain literal arguments start running while you are still writing the program, so put the reads you already know you need first, e.g. read(\"src/a.rs\") rather than read(path). EVERY tool you have is callable from the script (all async — use await; each returns the tool's text output):\n  callTool(name, args) — any tool by name with its normal JSON arguments, e.g. callTool(\"web_search\", {query: \"...\"})\n  tools.<name>(args), or <name>(args) as a global when the name is a plain identifier\n  listTools() — the names available\n  toolInfo(name) — { name, description, parameters } for one tool; use it before calling an MCP tool or playbook you have not used\n  parallel(items, fn, limit?) — run fn over items concurrently (default 8 at a time) and get the results in order, e.g. await parallel(urls, (u) => web_fetch({url: u}))\nShorthands with positional arguments:\n  bash(command, timeoutSeconds?)  read(path, opts?)  write(path, content)  edit(path, oldString, newString)  ls(path?, opts?)  grep(pattern, path?)  glob(pattern, path?)\nconsole.log output is returned above the script's return value. Each call goes through the same approval/gating as calling the tool directly. A failed call throws a catchable JS exception. A script cannot start another run_script.",
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

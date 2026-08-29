# Changelog

All notable changes to Vivianne are documented here.

## [1.7.3] — 2026-08-29

### Fixed

- **Provider errors ending the turn with no explanation** — when a provider returned a message whose `stop_reason` was `error` (out of credits, rate limited, bad key), that path emitted only `TurnEnd`, and the frontend store has no `turn_end` handler. The real reason existed solely in the log, so the agent appeared to abort instantly for no reason. Compounding it, `message_end` began with `if (streamingIndex === null) return s` — a final message with no in-flight stream was discarded outright, which is exactly the shape of a provider that fails before its first token. Both fixed; provider failures now surface as a red error in the transcript.
- **The agent denying capabilities it has** — the model's self-description came from a hand-written prompt block listing four tools while sixty existed, with web access described as `web_search`/`web_fetch` and no mention of the nine `browser_*` tools. Smaller models answered "I don't have browser tools" from that prose rather than scanning their own function list. Replaced with a capability map generated from the live, post-restriction tool set, plus a keyword router that names the exact tools for a request right after the user's message. A build-failing test now catches any tool added to the schema without a capability group.
- **camoufox never auto-starting on Windows** — the spawn ran `node` against `node_modules/.bin/camofox-browser`, which on Windows is npm's *shell* shim (`#!/bin/sh`). Node parsed it as JavaScript and died instantly, so every `web_preview`/`browser_*` call fell through to "not running" and the agent rationalised it as a missing capability. Now runs each package's real declared `bin` entrypoint.
- **camoufox failing on first request with a missing native binding** — the installer passes `--ignore-scripts` (to dodge `impit`'s `preinstall` hook), which also skipped `better-sqlite3`'s `install` script, leaving it with no compiled binding. `npm rebuild` now runs for the native modules specifically, and "installed" means the bindings exist rather than just the package.
- **`browser_snapshot` unable to page through a large DOM** — the server windows big accessibility trees and embeds a "call snapshot with offset=N" instruction, but the tool exposed no `offset` parameter, so anything past the first chunk was unreachable. Interaction errors also surfaced as raw JSON payloads instead of the actionable hint inside them.
- **Queued messages that were delivered but looked stuck** — the queue panel polls only while the agent is busy, so it froze on its last poll and kept rendering messages the run had already answered. Separately, a message queued after the loop's final steering drain had nothing left to pick it up until some later prompt.
- **Any non-text file failing to preview** — the All-files panel called a bare `read_to_string`, so every image, PDF, archive and binary rendered the same "stream did not contain valid UTF-8" error. Files are now classified and rendered by what they are.
- **Attachments decoding binaries into mojibake** — `File.text()` never fails (it substitutes replacement characters), so PDFs and Office files arrived as pages of garbage that looked like content to the model. Binaries are now detected and described instead.
- **Image input silently dropped on capable models** — vision support was guessed from substrings in the model id, which answers "no" for most OpenRouter models. OpenRouter publishes `architecture.input_modalities`; that is now read as authoritative, with the heuristic only as a fallback.
- **Terminals erroring on every launch** — the first `fit()` fired a resize before `cmd_terminal_spawn` resolved, so every terminal that started hidden logged "no terminal session for tab …" and spawned stuck at 80x24.
- **Model selection not surviving a restart** — nothing ever persisted it. Stored in its own file rather than `settings.json`, which is rewritten wholesale from a frontend snapshot and would clobber it.
- **`@` mentions and the file tree seeing only one workspace folder** — both were hardcoded to `folders[0]`, so a second folder was entirely unmentionable and invisible, and the depth/entry caps hid most of any real repo.
- **A re-render storm in the title bar** — a zustand selector returning `.filter(...)` built a fresh array on every read, so every store change anywhere re-rendered the whole bar.
- **`sync-versions.js` not updating the files the updater reads** — it synced only `package.json` files, leaving `Cargo.toml` and `tauri.conf.json` at the old version. A release would build, publish, and then be ignored by every client as "not newer", with no error anywhere.

### Added

- **Auto-update, changelog, and announcements.** The updater plugin was configured but never called from anywhere; it now checks silently at startup, installs with progress, and restarts. A "what's new" dialog fires once per upgrade from the embedded `CHANGELOG.md`, and a remote JSON feed can carry announcements between releases.
- **`create_document`** — real PDF, DOCX, XLSX and PPTX files. PDF and DOCX share one block vocabulary; XLSX keeps numbers numeric so they can be summed; PPTX is hand-written OOXML (no credible Rust crate exists) limited to title-and-bullet slides.
- **Tabbed docks.** The right sidebar and bottom bar are now the same tabbed container and can host any view, so switching to the file tree no longer discards what was already open.
- **A usage screen** — lifetime totals, streaks, a year-long activity heatmap with daily/weekly/cumulative modes, and an insights table, all from real recorded token counts.
- **Message actions** — timestamp, copy, edit, and revert under every bubble. Edit reverts to that message and refills the composer.
- **More built-in subagents** — `code-reviewer`, `debugger`, `test-writer`, `web-researcher`, `doc-writer`, each with a real tool allowlist.
- **Agent shell commands in Background Tasks.** `bash`/`powershell` run as one-shot processes, not PTY sessions, so the panel could never see them; they're now tracked from the tool events that were already being emitted.
- **Paste and drag-drop attachments**, sharing one classification path with the attach button.
- **`computer-use` and `camoufox-browser` skills** documenting the call sequences models most often get wrong.

### Changed

- **`computer` and the six `dip_*` tools merged into one tool** with an `action` enum. The split was the problem: the model had to pick the right tool out of seven before it could start, with no guidance on when to reach for which.
- **Playbooks and custom panels merged into one system** — presence of a `command` or `panel` field decides what a playbook is, instead of two parallel directories, stores and Settings tabs doing the same job.
- **One surface treatment across the app chrome.** The sidebar, both docks and the Settings dialog now share a single glass fill and corner radius; the previous `borderRadius: 3` resolved to 30px against this theme's base.

## [1.7.2] — 2026-08-25

### Fixed

- **Resize drag re-rendering the whole panel tree per pixel** — dragging the side panel or terminal edge used to write to the layout store (and synchronously persist to localStorage) on every `pointermove`, re-rendering up to 1200 unvirtualized rows or re-running the terminal's `fit()` + a resize IPC call on every tick. The drag now mutates the panel's DOM size directly and commits to the store once, on release.
- **Diff view computed while collapsed** — MUI's `Collapse` keeps its children mounted to measure them, so a tool call's O(N×M) line diff ran on every render of a long thread even when the diff was never opened. The diff body now only mounts (and only computes) once actually expanded.
- **Long threads growing DOM/memory without bound** — the message list mounted every message for the life of the session. Now virtualized (`virtua`), rendering only what's near the viewport.
- **Provider context cloned up to 4x per attempt** — the conversation context (including any inline images) was deep-cloned once per retry attempt, again per credential-pool candidate, and again on fallback — all before the single real network call that actually needed an owned copy. Now threaded as a shared reference through all three layers; exactly one real clone per attempt actually made.
- **Token-budget check re-walking the whole transcript every round-trip** — re-serialized tool call arguments/results (`.to_string().len()`) and re-summed the full history from message 0, twice per loop iteration, regardless of how little had changed. Now walks JSON values directly (no allocation) and caches the running total, only summing newly appended messages.
- **No cleanup on app exit** — quitting left every open terminal shell, running dev server, and running local model as an orphaned process, and dropped up to 5 seconds of unflushed usage/cost data. All four now shut down cleanly on exit.
- **Settings and session-list re-read from disk per call** — `settings.json` was re-read and re-parsed on nearly every tool-gating check and provider call; the session sidebar re-parsed every session file (up to 18MB each) after every turn. Both now cached, invalidated only on an actual write.
- **The agent mutex held for the whole turn** — a few read-only commands (session stats, model switch, debug status) and the post-turn autosave queued behind an in-flight turn's mutex (held up to 900s on an unanswered permission prompt) even though none of them touch anything the mutex actually protects. Now lock-free.
- **Terminal output flooding the webview** — one IPC event per 8KB PTY read meant a verbose build could push thousands of events/sec. Output is now coalesced onto a 12ms timer, cutting event volume 10-30x with no perceptible added latency.
- **Streaming shell output with no memory bound** — `bash`/`powershell`'s live output accumulated unboundedly for as long as the process ran, only ever getting trimmed after it finished. Now capped live, so a runaway command can't grow RSS without limit.
- **`usage_by_source.json` growing forever** — gained a new, never-pruned key for every session/subagent run ever; now bounded to a 90-day rolling window and written compact instead of pretty-printed.
- Startup no longer blocks first paint on a full parse of every session file just to prune empty ones — a cheap file-size check now skips anything too big to be empty, and the prune itself runs off the main setup path.
- Workspace-folder path checks (every `read`/`write`/`edit`/`bash` workspace-boundary check) no longer re-run `canonicalize` — a real, comparatively expensive syscall on Windows — on every single call.

### Changed

- Release builds now compile with full LTO, single codegen unit, and stripped symbols.

## [1.7.0] — 2026-08-23

### Fixed

- **The inference-stalling bug** — every successful turn had a real chance of hanging for a full 120 seconds before erroring out, no matter how many earlier fixes had been tried. Root cause: a serde field-naming mismatch (`CostBreakdown` needed `rename_all = "camelCase"`, and `MessageContent`'s enum needed `rename_all_fields` — `rename_all` alone only renames enum variant tags, not struct-variant fields) meant the terminal event of a successful stream failed to deserialize, and the listener silently swallowed the parse failure instead of logging it. Both fixed; the listener now logs any future decode failure instead of hiding it.
- **A 111MB read on every single turn** — the session list was being fully re-parsed (every message of every session file, up to 18MB each) after every turn just to refresh the sidebar. Replaced with metadata-only parsing and a cache invalidated at the one place sessions are actually written.
- **"New chat" not creating a real session** — clicking New Session reset in-memory state but never persisted anything, so the new thread had no id until the first message completed; any UI assuming otherwise broke. It's now persisted immediately as an empty "New chat" session, idempotently (rapid double-clicks reuse the same empty session instead of creating duplicates).
- **The Review panel showing "No textual diff" for real changes** — when the active workspace folder was a subdirectory of the git repo (not the true top level), a second `git diff` call resolved the file's pathspec against the wrong directory and silently returned nothing. Every git-backed command (`diff --numstat`, `diff <file>`, `status`) now resolves the actual repository root first.
- **`run_git`/git subprocess calls with no timeout** — a `repo_clone` against a slow or hung remote (or one that stops to ask for credentials on stdin) could block the tool call, and the turn with it, indefinitely. Now bounded to 120s.
- **The Subagents panel's turn/tool-call counter stuck at 0** while a subagent was actively running — it only ever got a real value once the subagent finished, so anything still in progress looked idle.
- Windows path-length crash in an unrelated local test, a stale `local_onnx` test assertion, and several stale doc comments/dead code paths left over from earlier refactors.

### Added

- **Real vision for images, with an automatic OCR fallback** — `read` on an image now shows the model the actual pixels (downscaled to a sane max edge, original bytes passed through unmodified when they're already small enough) instead of a text-only summary. When the currently selected model can't take image input at all, this is detected automatically and the image is OCR'd instead — text-only models get the text a sighted model would get from looking, rather than an unusable image block or a provider error.
- **Local OCR rebuilt on the real `RapidOCR` package** — the previous local fallback quietly ran an unrelated, mislabeled model. It now shells out to the actual, maintained `rapidocr` Python package (`pip install rapidocr`) instead of a hand-rolled reimplementation, with the HuggingFace-hosted tier still preferred first. Local OCR now also pauses any running local LLM process for the duration of the call to free RAM, resuming it afterward.
- **Persistent terminal sessions** — `terminal_open`/`terminal_send`/`terminal_read`/`terminal_signal`/`terminal_close`/`terminal_list`, backed by the same real PTY sessions the GUI's own terminal tabs use. Unlike `bash`/`powershell` (one-shot, no memory between calls), these keep shell state alive across calls — for an activated virtualenv, an interactive installer, a long-running dev server you poll, or anything needing real stdin.
- **Goal tracking** — `get_goal`/`create_goal`/`update_goal`, a durable-for-the-session objective distinct from the `todo_write` checklist (todos are steps; a goal is the point of the steps). Shown to the model every turn so it stays visible across compaction.
- **`list_tools`** — lets the model check exactly what's registered right now (built-in, MCP-provided, playbook-defined) and whether each is enabled, instead of only ever seeing its own static schema block.
- **Honest partial scans** — `grep`/`code_search` now say plainly when a scan was capped before finishing, instead of a partial search looking exhaustive and the model concluding "X isn't used anywhere" from an incomplete search.
- **Line-numbered `read`** — text files come back `cat -n`-style with the total line count stated, so the model can tell "I saw the whole file" from "this got cut off" without guessing.
- **Spill for truncated tool output** — when shell output is truncated to fit context, the trimmed middle is saved to disk and the truncation notice names the path, so the model can `read`/`grep` it back instead of it being gone for good. Old spill files are pruned automatically.
- **Context-window-scaled compaction** — memory/prompt-injection budgets now derive from the selected model's actual context window instead of fixed constants, so a small-context model (e.g. 8K) doesn't try to compact every single turn while a huge-context model doesn't throw away far more history than it needs to.
- **Pointer-style custom subagents** — a saved subagent profile can carry a preset `default_prompt`; the model can then delegate with just a profile name and a short `context` string instead of writing the whole task prompt every time. New `list_subagent_profiles` tool lets it check what's available first.
- **Real browser control** — `browser_snapshot`/`browser_click`/`browser_type`/`browser_press_key`/`browser_scroll`/`browser_evaluate`/`browser_read_console`/`browser_read_network`, built on camoufox-browser's REST API (click/type/scroll/evaluate were already there, just never wired up). `web_preview` was screenshot-only before this; now the agent can actually drive the page.
- **`set_panel`** — the agent can open/close GUI panels (Review, workspace files, the merged Preview panel, background tasks, terminal) so it can show the user what it's doing instead of only ever reacting to clicks.
- **First-launch prompt + background downloads for Camoufox and Docker** — both used to download silently (and block) on first use; now there's a one-time consent prompt, a non-blocking background download with real progress, and a Settings → Components section to check status or install later.
- **Real per-model cost tracking** — Settings → Usage showed $0.00 for every day since cost was never actually computed by any provider integration. Now priced from a real, sourced table (Anthropic/OpenAI/Google); a model outside that table shows honestly as unpriced instead of a fake zero.

### Changed

- **Gesture control removed from the main app** — it now lives entirely in its own standalone "Air Touch" app (`packages/gesture-control`); nothing about it remains in Vivianne proper.
- **The dead Rust `orchestrator` module removed** — a full rewrite of an already-retired TypeScript package that the frontend never actually called into.
- **Live Preview and Browser panels merged into one Preview panel** with a mode switch — Browser mode now also shows the agent's live camoufox session when it's actively driving one, replacing the separate Web Preview dialog.
- **`bash`/`powershell` always stream live output** to the GUI now, with labeled `[stdout]`/`[stderr]` sections, the exit code always appended, and head+tail truncation (so a build/test failure's summary at the end is never the part that gets cut). This used to be a separate `streaming_bash`/`streaming_powershell` tool pair the model had to choose to get live output — now there's one name per shell, not two.
- **Duplicate tools folded together**: `read_file`/`list_dir` merged into `read`/`ls`, `ask_user_question` removed in favor of the already-live, richer `question` tool.
- **`todo_write`** now rejects duplicate items and enforces at most one `in_progress` task at a time, and reports pending/in-progress/completed counts.
- Numerous internal performance fixes: reduced write amplification in the terminal output buffer, pooled HTTP clients instead of one per call, faster image re-encoding, parallelized independent git subprocess calls, and consolidated several duplicated truncation/skip-directory implementations into one each.

## [1.5.0] — 2026-07-20

### Added

- **AI Elements feature set** — ported from the AI Elements toolkit:
  - **Sources** — assistant replies now show a collapsible list of the web pages a `web_search`/`web_fetch` call actually drew from.
  - **Plan** — a `present_plan` tool for the agent to lay out a numbered step-by-step plan before acting.
  - **Sandbox** — a `sandbox_execute` tool that runs untrusted code in a real, network-isolated Docker container (`--network none`, memory/CPU/PID limits, read-only mount) rather than in-process.
  - **Snippet** — a `present_snippet` tool for showing a syntax-highlighted code snippet with a description, independent of a file edit.
  - **Web Preview** — a dockable panel showing a live screenshot of a URL the agent navigated to, backed by a real browser process (see camoufox-browser below) instead of a sandboxed iframe, so it also renders sites that block embedding (e.g. Google) and can open local `file://` HTML/PDF files directly.
  - **Queue** — a floating panel showing messages queued to steer the agent mid-run or follow up after it finishes, with per-item removal and an inline "queue a follow-up" input.
  - **Checkpoints** — hover any of your own messages to revert the conversation back to that point, removing everything after it (with a confirmation dialog showing exactly how many messages will be removed).
  - **Confirmations** — a `confirm_action` tool for the agent to ask a plain yes/no question before a risky action, reusing the existing question/answer request pattern.
- **camoufox-browser integration** — web preview, web search, and web_fetch now route through [camoufox-browser](https://github.com/jo-inc/camofox-browser), a real (fingerprint-spoofed) Firefox process, instead of an iframe or plain HTML scrape. Auto-launches (`npx @askjo/camofox-browser`) the first time it's needed if it isn't already running.
- **One-click Docker install** — Settings → Engine → Sandbox now shows Docker Desktop's status and, if it isn't installed, offers to download and install it (fetched fresh from docker.com at install time, not bundled — keeps the installer small and always gets Docker's current signed build). Requires the user's explicit confirmation in-app, then Windows' own admin-elevation prompt.
- **OpenAI Responses API** — direct OpenAI calls (`provider: "openai"`) now speak `/v1/responses` instead of the legacy Chat Completions endpoint, with true incremental streaming, reasoning summaries, and correct function-call/function-call-output formatting. Every OpenAI-*compatible* third-party host (OpenRouter, Groq, xAI, DeepSeek, Together, Fireworks, Cerebras, NVIDIA NIM, local servers) stays on Chat Completions, since none of them implement the Responses endpoint.
- **Refreshed welcome screen** — replaced the placeholder `viv --model claude-sonnet` CLI command box (a leftover from before Vivianne was a GUI app) with the Vivianne logo and four clickable quick-start suggestions (explore the codebase, find/fix a bug, add a feature, refactor) that pre-fill the composer.

### Fixed

- **MCP connections failing with 406 / SSE parse errors** — the Streamable-HTTP transport wasn't sending the `Accept: application/json, text/event-stream` header the 2025-03-26 MCP spec requires, and couldn't parse SSE-formatted response bodies. Also added `Mcp-Session-Id` header tracking.
- **Automations firing at the wrong time** — the cron scheduler was computing `next_run` against UTC directly, silently reinterpreting every cron field as UTC instead of the user's local timezone (an 11:00 automation was firing at 16:30 for an IST user). Fixed by converting to local time for the schedule math, then back to UTC for storage.
- **Stale model list after adding/removing an API key** — the available-models list only ever refreshed once at app start; adding or deleting a key now refreshes it immediately.
- **camoufox-browser tab creation returning HTTP 400** (`"userId and sessionKey required"`) — the `POST /tabs` request was missing the (undocumented-as-required) `sessionKey` field.

### Changed

- Removed the dead `ExtensionUIRequest`/`sendUiResponse` scaffolding (never actually wired to any backend command) in favor of the working `confirm_action` tool.
- Diff panel now does real syntax highlighting (via Shiki) instead of plain monospace text.

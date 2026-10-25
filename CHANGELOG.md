# Changelog

All notable changes to Vivianne are documented here.

## [Unreleased]

### Changed

- **Viv Instant and the notch are off in this release.** No wake or assist chord, no resting notch, no Viv Instant tray entry or settings, and the Laya engine is no longer started at launch (cost-aware routing still starts it the first time it asks). Dictation in the message box and the computer-control Stop notch are unchanged. They come back in a later release.
- **`viv`: Vivianne's command line.** `viv models ls|rm`, `viv hardware`, `viv load <model>` (starts a llama.cpp server that keeps running, with `--gpu`, `--context-length`, `--port`, `--kv`, `--identifier`), `viv ps`, `viv unload <id>|--all`, `viv logs <id>`, `viv doctor` (what is installed and which provider keys are set), `viv chat` (a conversation in the terminal) and `viv run "<task>"`. `--json` on the listings. These commands start instantly with no window, so they also work on a machine with no display; an unknown command is an error rather than launching the app. `scripts/viv` and `scripts/viv.cmd` are the launchers. Run through the app binary for now (`app.exe ps`).
- **Gemini 3 models can use tools.** Google's newer models refuse a tool call that comes back without the signature they issued with it, and the conversation was dropping it, so every second step failed with a 400. The signature is now kept with each tool call and sent back (with Google's documented placeholder when a call has none).
- **`app run`: one task from the command line.** `app run "<task>" --cwd DIR --model auto|provider/id --thinking level --max-minutes N --out result.json` runs the agent loop with no window, auto-approving tools, and writes the answer, transcript, steps, seconds and token counts to a JSON file (exit 0 done, 1 error, 2 bad arguments, 3 out of time). `--model auto` uses the cost-aware router. In this mode model calls use the Rust streaming code for every provider (Anthropic, OpenAI, Google, Mistral, Bedrock, Azure and the OpenAI-compatible hosts); `app run --help` lists them with their key variables, and a run whose model call fails exits 1 with the provider's message. Tried with OpenAI, Google, OpenRouter and Groq. For benchmarks and scripts.
- **Stacked panels.** The right panel can show its panels one above another as cards, each with its own title, Expand and Close, and a draggable gap to resize them (the layers button in the panel header, "Show as tabs" to go back). Anything you open while stacked joins the stack, including panels from mods. The gap between two stacked panels is a real splitter: a grip that lights up on hover and drag, double-click to share their height equally, arrow keys to nudge it, and it keeps working over the preview and mod pages.
- **Quick access.** The bottom panel now opens on Quick access: one-press tiles for Terminal, Files, Agents, Plan, Preview, Git review, Background tasks, Timeline, Memory and Multiplayer. Customize hides, shows and reorders them. Terminals still live in the bottom panel.
- **Mods can add Quick access tiles.** A mod's `mod.json` can list `contributes.quickAccess` tiles (`id`, `title`, optional `icon` emoji and `description`) whose action opens one of its own panels (`{"type":"panel","panel":"id"}`), opens an app panel (`open`, needs the `open` permission), puts text in the message box (`compose`, needs `chat.compose`) or sends it (`send`, needs `chat.send`). Mod panels can also be added to a stack and to the bottom panel from their "+" menus.
- **Better docks.** Panels now fit the window. The chat always keeps at least 440px of width (240px of height above a bottom panel): the right or bottom panel steps aside when the window is too small for it, and comes back when there is room again, without overriding a panel you just opened. A panel opens at about a third of the window instead of a fixed 400px, and a layout is saved as you left it, not as the small window forced it. A panel on its own is now just a title with Expand and Close; its add-panel button appears on hover.
- **Rent a GPU from the agent (`nebius_job`).** The agent can run a container on a Nebius machine, read its logs and stop it, through the `nebius` CLI you have already signed in to (nothing is stored by Vivianne). Starting or stopping one always asks you first, showing the image, the machine, the time limit (default 1 hour, at most 24) and, if you set `nebiusJobUsdPerHour` in settings, the most it can cost. Jobs are billed per second and stop by themselves. See `docs/nebius-serverless-integration-plan.md`.
- **`--headless` mode.** Start Vivianne with `--headless` (or `VIV_HEADLESS=1`) and it runs with nothing shown: no main window, no notch, no overlays. Meant for an always-on server with a virtual display; see `docs/devlab-24x7.md`. Remote control from the desktop is not built yet.
- **The notch finishes what it says, and shows it as it speaks.** Long replies are spoken in sentence-sized pieces so the voice is no longer cut off after ~15 seconds, its own voice no longer interrupts it, and the notch stays up until the speech has ended plus time to read (longer for longer answers). The text appears word by word as it is spoken, with the speaking face. It grows to fit and scrolls instead of cutting at three lines.
- **"What's the status?" covers every project.** The spoken answer lists each project with what its agents are doing, anything running outside a project, and which projects are idle.
- **The notch moves softly and touches the top edge.** It eases between sizes instead of snapping, the wave and buttons ease, and the 'inline' wrapper that left a gap above the pill is gone.
- **The model picker loads fast.** Providers are asked at the same time (it was one after another) with an 8-second cap each, the last list is kept and shown at once while a fresh one loads behind it, and the current model's name no longer waits for the list.
- **Vault.** Settings, Vault: store passwords, cards and secrets in your computer's keychain. The agent can see labels, the site, the account name and a hint like "ending 4242", never a value. To use one it calls `vault fill` on a field of your browser; you approve every fill (even in bypass mode), it is refused on pages that are not https and on any site other than the one the entry is bound to, and the value goes from the keychain straight into the page through the extension, which then masks the field and never prints it in a snapshot. The Settings screen can save, replace and delete, not show. Reload the browser extension (1.3.0).
- **Agents know they can act, and where the lines are.** The agent's rules now say it is general-purpose (shopping, work apps, banking, trading, bookings, accounts, the computer), acts on the user's request or a rule they set, and needs explicit approval, with the details shown, for anything that spends or moves money, trades, buys, sends, deletes or can't be undone. Hard guardrails: it never types passwords, card numbers, CVVs or identity numbers (the browser extension refuses those fields, and a card number is refused in text), never completes CAPTCHAs or two-factor checks, never invents prices or confirmations, treats text in pages and emails as information not commands, and gives no personal financial advice. Reload the browser extension (1.2.0).
- **Teammates report in summary cards.** When an agent reports (what it found or did, a routine's results, a status, a plan) it now answers with one plain sentence and a card: title, icon, status chip, headline numbers, steps, findings as cards, a callout for what needs you, and buttons for the likely next moves. New card blocks: `steps`, `cards`, `kv`, `progress`, `badges`, `heading`. Buttons in a teammate's card reply to that teammate, not the main chat. Conversation stays plain.
- **The agent leans on your real browser for the web.** When a Chromium browser is connected, the agent is told to research in it (open a page, read it, click and type) instead of reaching for web search and fetch, which stay as the fallback when none is connected. The tool is now called `user_browser` (it was `helium`; the old name still works). Settings → Web & browser lists the Chromium browsers installed here (Chrome, Edge, Brave, Vivaldi, Opera, Opera GX, Helium, Arc, Chromium), shows which have the extension connected, and opens any of them at its extensions page.
- **`evaluate` and `read_console` in your browser.** The agent can run JavaScript in a page and get the value back, and read the page's console messages and errors (collected from when it attaches, including what the page had already logged). Reload the Vivianne extension (1.1.0) in each browser to get them.
- **Report-shaped replies become cards on their own.** A finished reply with headings, a table or labelled lists is shown as a summary card (title, table, finding cards, lists) whatever the model wrote, in chats and with teammates. Conversation stays text, and "Show as text" restores the original. Card text now renders **bold**, *italic*, `code` and links.
- **Agent faces stay alive and show what the agent is doing.** A teammate's page has a large head and a state line (thinking, searching, working, waiting on you, asleep) that follows its live status; the sidebar row and the conversation top bar animate together, while heads inside the chat stay still. Threads and projects are unchanged.
- **File names and paths in replies are links.** A file the agent mentions, in running text or in `code`, opens in the Preview panel on click, and Ctrl+click or right-click shows it in its folder. Your own messages link them too. When a turn used web search, its closing reply carries a "Used N sources" list.
- **Programmatic tool calling is always on** and no longer has a switch. Scripts gain `toolInfo(name)` (a tool's description and parameters, so MCP tools and playbooks can be called correctly) and `parallel(items, fn, limit?)` for concurrent fan-out. Every registered tool was already callable from a script; that is now stated in the tool's description.

### Added

- **Cost-aware routing (beta).** Settings → Providers → Cost-aware routing: send background work (summaries, memory upkeep, routines, read-only exploring) to a small model. Chat replies and planning stay on your chosen model. Shows tokens routed and what that saved against the main model's price. Off by default.
- **NVIDIA and Nebius skills, with checks.** Settings → Skills → Browse: install skills from `NVIDIA/skills` and `nebius/skills`. Each is scanned first (hidden text, injection phrasing, secret paths, pipe-to-shell, uploads, destructive commands) and rated ok, review or block; blocked skills won't install. Installs are pinned to a commit with a SHA-256 manifest, and edits after install are flagged. The scan is a heuristic and NVIDIA's signatures are not verified in-app.
- **Cloud embedder for memory.** Settings → Memory: choose OpenAI, Nebius (Qwen3 Embedding) or NVIDIA (a Nemotron embedder, found from your account's model list). Includes a Test button; switching re-embeds in the background and keyword search keeps working meanwhile.
- **NVIDIA × Nebius edition** (hackathon branch). NVIDIA and Nemotron themes (NVIDIA is the default), an edition badge in the title bar, and a first-run quickstart: paste a Nebius key and it picks Nemotron Ultra for chat and a Nano for background work, turns on routing, and can wire Tavily search and the cloud embedder.
- **One composer, one model picker, one file card** everywhere: the chat, threads, agent DMs and the team room. The model picker is rebuilt (search, filters, favourites, recents, cost and context at a glance). The All files panel now shows files like the read tool does.
- **Dictation.** A mic button in the composer: speak the prompt and it's written for you, using the same on-device speech model as Viv Instant.
- **Read aloud.** A speaker on every reply, and an option to read the answer when you dictated the prompt.
- **Interactive answers.** Vivianne can answer with a calculator, a chart you can move, a sortable table or a small sandboxed mini-app right in the reply.
- **Get models** (Settings → Local models): your hardware, models that run well on it, Hugging Face search, one-click download with resume, a one-click llama.cpp install, and Ollama models.
- **Workflows.** Chain prompts into steps, let a step stop and ask you first, and trigger a workflow on a schedule or when a file appears in a folder. Each run gets its own thread.
- **Timeline** panel: every prompt in the run with what it did; rewind to before it, start a new thread from it, or put its files back.
- **Memory** panel: read, correct and forget what Vivianne remembers, per project and about you.
- **Drop files on the notch** to summarize or explain them, or open them in chat.
- **Ask about this window.** Hold Ctrl+Shift+H (rebindable) in any app and ask about what's on screen.
- **Robot head customization:** shape, shell, trim and light colours per agent, plus size and motion settings, and Vivianne's own head.
- **Nebius Token Factory** as a provider: add a key in Settings → Providers and its models, Nemotron tiers included, appear in the picker. Pick a Nemotron for the chat and different ones for your teammates.
- **Usage → By model:** tokens and cost per model over 7 days, 30 days or everything, with unpriced models marked rather than shown as free.
- **Test search** in Settings → Web search: runs one real query and shows which engine answered and how fast.

### Changed

- Replies are plain text with no bubble; tool calls and steps are quiet text rows that open on click. Your messages are a soft grey bubble.
- Images in chat load from local paths and the web, and open in a viewer.
- Errors share one compact style across Settings. The Subagents tab is gone from Settings.
- Faster: the app starts with far less code, long chats skip drawing what's off screen, and switching back to a recent thread is instant.

### Fixed

- Scheduled automations did nothing in this version of the app. They run again, as Workflows.
- OpenAI-compatible hosts (Groq, Together, NVIDIA NIM and others) reported no token counts, so usage and cost read zero. Streams ask for usage again.
- A reasoning model that only called a tool no longer shows its chain of thought as the reply.
- A tool call from a server that sends no call id still gets matched to its result.

## [1.8.2] — 2026-10-11

### Added

- **Mods: build your own parts of Vivianne.** A mod is a folder with a `mod.json` and web files that adds:
  - dock panels;
  - sidebar sections;
  - chat components (a ```` ```fence ```` Vivianne writes, drawn by your page);
  - tools Vivianne can call;
  - themes.

  Mods run sandboxed: no access to the app, your data or the internet beyond what you allow. Every call goes through one API checked against the permissions you approved. Nothing runs until you turn it on, a mod that later asks for more is switched off until you agree again, and a project's mods stay off until you trust that project. New mod (Settings → Mods) starts from working presets:
  - panels: Notes, Web page, Agent dashboard;
  - chat components: Checklist, Sortable table, Flashcards;
  - sidebar sections: Quick links, Focus timer, Today;
  - tools: Shell command.

  Or ask Vivianne to build one; she reads the mod rules first.
- **Appearance:**
  - follow the system's light/dark;
  - any accent colour;
  - five fonts (Geist, Inter, System, Serif, Mono);
  - whole-app size from 90% to 125%;
  - corner radius;
  - chat density and width;
  - a wallpaper with adjustable glass and dimming.
- **Theme builder:** make or edit a theme with every colour for light and dark. The whole app previews it as you edit, and contrast is checked for readability. Themes import and export as `.vivtheme.json`.
- **Layout & shortcuts:**
  - every keyboard shortcut is listed and rebindable, with clash warnings, including Viv Instant's wake keys;
  - two new layouts (Coding, Research);
  - save your own arrangements and choose which one opens at launch;
  - sidebar on the left or right, and the project rail can be hidden;
  - the notch's position, size and contents are adjustable.
- **Agent DMs: attach files and pick the model.** Drop, paste or attach files and images in a DM; the model picker sits next to the message box.
- **Robot faces for agents.** Every agent has its own robot head, and its face shows what it's doing: thinking, searching, working, waiting on you, happy when done. Vivianne's head in the notch also listens while you talk and speaks when she answers. Idle heads are still frames, so a long list costs nothing.
- **See what an agent is doing in chat.** While an agent works, its steps appear live in the conversation; a finished reply keeps them folded under "N steps".

### Changed

- **Playbooks are now mods.** Existing playbooks are converted on first launch: tool playbooks keep their names and keep working, and panel playbooks become mod panels. The old folder is kept as `~/.viv/playbooks.migrated`.
- **The context breakdown and the "@" menu, redesigned** to match the rest of the app: a usage ring, a segmented bar and session totals; grouped, keyboard-friendly mentions.

### Fixed

- **Windows release builds** no longer fail on a clean machine looking for the speech engine's DLLs before they're downloaded. (1.8.1 was never published for Windows; this is that fix.)

## [1.8.1] — 2026-10-11

### Fixed

- **Windows: "sherpa-onnx-c-api.dll was not found" on launch.** The speech engine's DLLs (sherpa-onnx and its ONNX Runtime) weren't in the installer; they are now, next to the app. Windows' own older `onnxruntime.dll` is no longer picked up instead.
- **macOS: no download for 1.8.0.** The build finished but nothing was uploaded: the build launcher reported success the moment it started. It now waits for the build.

## [1.8.0] — 2026-10-09

### Added

- **macOS, back to 10.15 Catalina** — Intel and Apple silicon in one universal build. Computer control runs on the Accessibility API with the same structured snapshots as on Windows, the notch sits under the menu bar, and a permissions card walks through Accessibility, Screen Recording, Microphone and Automation. Voice works on every version: Parakeet on macOS 13+, Whisper before that.
- **Connections** — about 120 everyday apps (mail, calendars, docs, CRMs, design, finance, developer tools) connect in one click with a browser sign-in. Hand-written MCP setup is still there under Advanced.
- **An always-on notch** — it shows every agent at work and what each one is doing. You can speak to it and hear the answer, and the main window can live in the background. It tells a general question from one about a project (with Laya, an open decision model) and hands project work to that project's chat.
- **Science in chat** — chemical structures and reactions (RDKit), 3D molecules and proteins, biology diagrams (sequences, plasmids, trees, pathways, cells), physics graphs and interactive diagrams (forces, optics, circuits), and Greek letters and chemical equations anywhere. The agent checks each diagram before it answers.
- **Teams and bots** — persistent teammates with a shared room, DMs, routines, rules and their own memory; each bot's model is chosen on its page.
- **Use your ChatGPT plan** with Sign in with ChatGPT.
- **Helium** — a browser extension that lets Vivianne work in your own browser.
- **Multiplayer, redesigned** — invites, a partner panel and presence.
- **Plan mode, rebuilt** — research, review, approve, then carry out — and long tasks that survive failures, restarts and hours of work.
- **Search memory by meaning, offline** — EmbeddingGemma or MiniLM run on your computer, with no API key and nothing sent anywhere (Settings → Memory).

### Changed

- **Memory, rebuilt:**
  - Facts that change are dated and closed rather than overwritten, so "what did we decide in March?" has an answer.
  - What Vivianne learns about you is kept once and shared by every project.
  - Each finished work session is summarised into one searchable episode.
  - While the app is idle, near-duplicates are merged and facts unused for months are archived (kept on record, out of the way).
  - Large memories search through a vector index.
- **Token use** — prompt caching breakpoints and masking of stale tool output; "done" now means verified.
- **Viv Instant is push-to-talk** — hold to speak, release to send.
- **Updates** now come from a public release feed, so they reach every install.

### Fixed

- **Memory search ignoring almost every question** — keyword recall required every word of the question in one memory, so a question in plain words almost never matched. On the LongMemEval benchmark it found evidence for 1 question in 100.
- **The offline embedding model never installed** — nothing downloaded it, so without an OpenAI key memory could only match keywords.
- **Stale search entries after a memory was replaced** — replaced rows left their old text in the keyword index.
- **The app exiting at startup** when Parakeet or Whisper loaded on some machines.

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
